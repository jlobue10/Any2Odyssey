//! Typefaces of the system for the letters the program's own do not have.
//!
//! The built-in faces (Fira Sans, Libre Baskerville and what egui brings)
//! hold Latin, Greek and Cyrillic. A window title or a file name in
//! Chinese, Japanese, Korean, Hebrew or Arabic was drawn as a row of boxes:
//! such windows could not be told apart in the source list, and the viewer
//! said "Loaded" and a row of boxes.
//!
//! Nothing is read at start. Every pass, the text that was painted is looked
//! through for letters not seen before; those that the typefaces in use lack
//! are looked for in the system's own, on a thread of their own, and a file
//! that has them is added behind the built-in ones: one file for a script,
//! read once, when a letter of it is first shown. The page is drawn again
//! as soon as the file is in.
//!
//! What this does not do: egui lays text out one letter after another from
//! left to right and joins nothing. Hebrew and Arabic therefore come out
//! with their letters in the order they are stored, which is the reverse of
//! how they are read, and Arabic in its unjoined forms. The names can be
//! told apart; they cannot be read fluently.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{FontData, FontFamily, FontId};

/// The most files that are ever added: what they hold stays in memory.
const MOST_FILES: usize = 8;
/// A file larger than this is not read (the largest of Windows' own for
/// these scripts is 37 MB, Noto Sans CJK 20 MB).
const LARGEST_FILE: u64 = 64 * 1024 * 1024;

/// One face in a file of typefaces.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FontFile {
    pub path: PathBuf,
    /// Which face of a collection (`.ttc`); 0 otherwise.
    pub index: u32,
}

impl FontFile {
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    pub(crate) fn new(path: impl Into<PathBuf>) -> FontFile {
        FontFile {
            path: path.into(),
            index: 0,
        }
    }
}

/// The languages that write with Chinese characters, each with shapes of
/// its own for the same character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cjk {
    Japanese,
    Korean,
    Simplified,
    Traditional,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Script {
    Han,
    Kana,
    Hangul,
    Bopomofo,
    Hebrew,
    Arabic,
    Thai,
    Indic,
    Other,
}

pub(crate) fn script(letter: char) -> Script {
    match letter as u32 {
        0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F | 0x1B000..=0x1B16F => Script::Kana,
        0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF | 0xFFA0..=0xFFDC => {
            Script::Hangul
        }
        0x3100..=0x312F | 0x31A0..=0x31BF => Script::Bopomofo,
        0x2E80..=0x2FDF
        | 0x3000..=0x303F
        | 0x3190..=0x319F
        | 0x31C0..=0x31EF
        | 0x3200..=0x9FFF
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF65
        | 0xFFE0..=0xFFEF
        | 0x20000..=0x3FFFF => Script::Han,
        0x0590..=0x05FF | 0xFB1D..=0xFB4F => Script::Hebrew,
        0x0600..=0x06FF | 0x0750..=0x077F | 0x0870..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => {
            Script::Arabic
        }
        0x0E00..=0x0E7F => Script::Thai,
        0x0900..=0x0DFF => Script::Indic,
        _ => Script::Other,
    }
}

/// Whose shapes to prefer for `letter`, shown together with `beside`.
///
/// A Chinese character alone does not say which language it is written in.
/// The user's own language decides where it is one of these; otherwise the
/// letters beside it do (Japanese and Korean have letters of their own), and
/// without those it is taken for Chinese.
pub(crate) fn cjk_of(letter: char, beside: &[char], user: Option<Cjk>) -> Option<Cjk> {
    match script(letter) {
        Script::Kana => Some(Cjk::Japanese),
        Script::Hangul => Some(Cjk::Korean),
        Script::Bopomofo => Some(Cjk::Traditional),
        Script::Han => Some(user.unwrap_or_else(|| {
            let has = |wanted| beside.iter().any(|c| script(*c) == wanted);
            if has(Script::Kana) {
                Cjk::Japanese
            } else if has(Script::Hangul) {
                Cjk::Korean
            } else {
                Cjk::Simplified
            }
        })),
        _ => None,
    }
}

/// Where the system keeps its typefaces.
pub(crate) trait Finder: Send + Sync + 'static {
    /// The user's language, where it is one that writes Chinese characters.
    fn user(&self) -> Option<Cjk>;
    /// The files that may have `letter`, the most wanted first.
    fn candidates(&self, letter: char, cjk: Option<Cjk>) -> Vec<FontFile>;
}

/// The bytes of `file` if they are a typeface egui can use.
///
/// egui panics over a file it cannot read as one, so nothing is handed to it
/// that the parser it uses has not accepted here first.
fn read(file: &FontFile) -> Option<Vec<u8>> {
    let size = std::fs::metadata(&file.path).ok()?.len();
    if size == 0 || size > LARGEST_FILE {
        return None;
    }
    let bytes = std::fs::read(&file.path).ok()?;
    ab_glyph::FontRef::try_from_slice_and_index(&bytes, file.index).ok()?;
    Some(bytes)
}

fn has(face: &ab_glyph::FontRef<'_>, letter: char) -> bool {
    use ab_glyph::Font as _;
    face.glyph_id(letter).0 != 0
}

/// Where a face's baseline comes to lie in a row of text, in parts of the
/// text's size from the row's top, when egui draws the face as the first
/// of its family: at its ascent. A face further back in the family is
/// centred on the first one's line by its own height, line gap included,
/// so that its baseline lies where chance has it: Yu Gothic, which asks for
/// half a line of gap, stood a quarter of the text's size above the Latin
/// letters beside it.
fn line_of(face: &impl ab_glyph::Font) -> Option<(f32, f32)> {
    let em = face.units_per_em().filter(|em| *em > 0.0)?;
    let ascent = face.ascent_unscaled() / em;
    let height = (face.ascent_unscaled() - face.descent_unscaled() + face.line_gap_unscaled()) / em;
    Some((ascent, height))
}

/// The line of the built-in text face.
fn own_line() -> Option<(f32, f32)> {
    static LINE: std::sync::OnceLock<Option<(f32, f32)>> = std::sync::OnceLock::new();
    *LINE.get_or_init(|| {
        let face = ab_glyph::FontRef::try_from_slice(super::theme::UI_FONT_FILE).ok()?;
        line_of(&face)
    })
}

/// How far down, in parts of the text's size, a system face has to be moved
/// for its baseline to lie on the built-in face's.
fn baseline_shift(own: Option<(f32, f32)>, system: Option<(f32, f32)>) -> f32 {
    let (Some((own_ascent, own_height)), Some((ascent, height))) = (own, system) else {
        return 0.0;
    };
    let shift = own_ascent - (ascent + 0.5 * (own_height - height));
    // A file with numbers that make no sense is left where it is.
    if shift.is_finite() && shift.abs() <= 0.5 {
        shift
    } else {
        0.0
    }
}

#[derive(Default)]
struct State {
    /// Every letter that was looked at. None is looked at twice.
    seen: HashSet<char>,
    /// The letters the typefaces in use lack, waiting to be looked for.
    wanted: Vec<char>,
    /// A thread is looking.
    searching: bool,
    /// The files that were added.
    added: Vec<FontFile>,
    /// The files that did not have a letter, by the letter's block of 256:
    /// a file is not read again for the neighbours of a letter it lacks.
    refused: HashSet<(FontFile, u32)>,
}

type Log = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone)]
pub(crate) struct SystemFonts {
    state: Arc<Mutex<State>>,
    finder: Arc<dyn Finder>,
    log: Log,
}

impl SystemFonts {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Nothing is being looked for and nothing waits to be.
    #[cfg(test)]
    pub(crate) fn settled(&self) -> bool {
        let state = self.state();
        !state.searching && state.wanted.is_empty()
    }

    /// The files added so far.
    #[cfg(test)]
    pub(crate) fn added(&self) -> Vec<FontFile> {
        self.state().added.clone()
    }

    /// Note the letters of `shape` that have not been seen before.
    fn note(shape: &egui::Shape, seen: &mut HashSet<char>, fresh: &mut Vec<char>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| Self::note(s, seen, fresh)),
            egui::Shape::Text(text) => {
                for letter in text.galley.text().chars() {
                    // The built-in faces have all of ASCII and Latin-1. Only
                    // letters and digits are looked for: a sign that no
                    // built-in face has (the title bar's own square is one,
                    // drawn with egui's stand-in, which is a square) must
                    // not have a file read at every start, and once a file
                    // is in for a script's letters its signs come from it.
                    if letter as u32 >= 0x100 && letter.is_alphanumeric() && seen.insert(letter) {
                        fresh.push(letter);
                    }
                }
            }
            _ => {}
        }
    }

    /// Look for the letters that wait, until none does. Runs on its own
    /// thread: a file of 20 MB is read here, and on Linux a program is run.
    fn search(self, ctx: egui::Context) {
        loop {
            let mut letters = {
                let mut state = self.state();
                if state.wanted.is_empty() {
                    state.searching = false;
                    return;
                }
                std::mem::take(&mut state.wanted)
            };
            while let Some(&letter) = letters.first() {
                match self.find(letter, &letters) {
                    Some((file, bytes)) => {
                        // Kept until the program ends, like the built-in
                        // faces: egui copies bytes it is given to own each
                        // time it rebuilds its fonts, and borrows these.
                        let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
                        let mut shift = 0.0;
                        if let Ok(face) =
                            ab_glyph::FontRef::try_from_slice_and_index(bytes, file.index)
                        {
                            letters.retain(|c| !has(&face, *c));
                            shift = baseline_shift(own_line(), line_of(&face));
                        }
                        letters.retain(|c| *c != letter);
                        let name = {
                            let mut state = self.state();
                            state.added.push(file.clone());
                            format!("system-{}", state.added.len())
                        };
                        let mut data = FontData::from_static(bytes);
                        data.index = file.index;
                        data.tweak.y_offset_factor = shift;
                        ctx.add_font(FontInsert::new(&name, data, families()));
                        (self.log)(&format!(
                            "Letters the built-in typefaces lack (U+{:04X}) are drawn with {}",
                            letter as u32,
                            file.path.display()
                        ));
                    }
                    None => {
                        letters.remove(0);
                    }
                }
            }
            ctx.request_repaint();
        }
    }

    /// The first file of the system's that has `letter` and can be used.
    fn find(&self, letter: char, beside: &[char]) -> Option<(FontFile, Vec<u8>)> {
        let block = letter as u32 >> 8;
        let cjk = cjk_of(letter, beside, self.finder.user());
        for file in self.finder.candidates(letter, cjk) {
            {
                let state = self.state();
                if state.added.len() >= MOST_FILES {
                    return None;
                }
                // One that was added and lacks the letter, or the letter
                // would not be lacking; or one that lacked its neighbour.
                if state.added.contains(&file) || state.refused.contains(&(file.clone(), block)) {
                    continue;
                }
            }
            // Whether it has the letter, and whether it has another letter
            // of the same block that waits beside it.
            let (mut wanted, mut later) = (false, false);
            let bytes = read(&file).inspect(|bytes| {
                if let Ok(face) = ab_glyph::FontRef::try_from_slice_and_index(bytes, file.index) {
                    wanted = has(&face, letter);
                    later = beside
                        .iter()
                        .any(|c| *c as u32 >> 8 == block && has(&face, *c));
                }
            });
            match bytes {
                Some(bytes) if wanted => return Some((file, bytes)),
                // Read again when that letter's turn comes.
                _ if later => {}
                _ => {
                    self.state().refused.insert((file, block));
                }
            }
        }
        None
    }
}

/// Behind the built-in faces of every family the pages use.
fn families() -> Vec<InsertFontFamily> {
    [
        FontFamily::Proportional,
        FontFamily::Monospace,
        super::theme::display_family(),
    ]
    .into_iter()
    .map(|family| InsertFontFamily {
        family,
        priority: FontPriority::Lowest,
    })
    .collect()
}

impl egui::Plugin for SystemFonts {
    fn debug_name(&self) -> &'static str {
        "system-fonts"
    }

    fn on_end_pass(&mut self, ctx: &egui::Context) {
        // Everything painted in this pass: the page, and the areas over it
        // (a drop-down list, a tooltip, a menu).
        let mut layers = vec![egui::LayerId::background(), egui::LayerId::debug()];
        ctx.memory(|memory| layers.extend(memory.layer_ids()));
        let mut fresh = Vec::new();
        {
            let mut state = self.state();
            ctx.graphics(|graphics| {
                for layer in &layers {
                    if let Some(list) = graphics.get(*layer) {
                        for entry in list.all_entries() {
                            Self::note(&entry.shape, &mut state.seen, &mut fresh);
                        }
                    }
                }
            });
        }
        if fresh.is_empty() {
            return;
        }
        let font = FontId::proportional(14.0);
        ctx.fonts_mut(|fonts| fresh.retain(|letter| !fonts.has_glyph(&font, *letter)));
        if fresh.is_empty() {
            return;
        }
        let start = {
            let mut state = self.state();
            state.wanted.append(&mut fresh);
            !std::mem::replace(&mut state.searching, true)
        };
        if start {
            let (fonts, ctx) = (self.clone(), ctx.clone());
            let spawned = std::thread::Builder::new()
                .name("system-fonts".into())
                .spawn(move || fonts.search(ctx));
            if spawned.is_err() {
                // No thread to be had: the letters stay boxes, as before.
                let mut state = self.state();
                state.searching = false;
                state.wanted.clear();
            }
        }
    }
}

/// Let `ctx` draw letters its own typefaces lack with those of the system.
/// `log` is told once for each file that is added.
pub(crate) fn install(ctx: &egui::Context, log: impl Fn(&str) + Send + Sync + 'static) {
    install_with(ctx, os::System::new(), log);
}

pub(crate) fn install_with(
    ctx: &egui::Context,
    finder: impl Finder,
    log: impl Fn(&str) + Send + Sync + 'static,
) -> SystemFonts {
    let fonts = SystemFonts {
        state: Arc::default(),
        finder: Arc::new(finder),
        log: Arc::new(log),
    };
    ctx.add_plugin(fonts.clone());
    fonts
}

#[cfg(windows)]
pub(crate) mod os {
    use super::{Cjk, Finder, FontFile, Script};
    use std::path::PathBuf;

    /// The Fonts folder of Windows. Windows 10 and 11 come with a typeface
    /// for each of these scripts in every edition; the names after the first
    /// are those of Windows 7 and 8, and of optional features.
    pub(crate) struct System {
        folder: PathBuf,
        user: Option<Cjk>,
    }

    // The medium weight first: Yu Gothic's regular is thinner than the
    // built-in text face beside it.
    const JAPANESE: &[&str] = &["YuGothM.ttc", "YuGothR.ttc", "meiryo.ttc", "msgothic.ttc"];
    const KOREAN: &[&str] = &["malgun.ttf", "gulim.ttc"];
    const SIMPLIFIED: &[&str] = &["msyh.ttc", "msyh.ttf", "simsun.ttc"];
    const TRADITIONAL: &[&str] = &["msjh.ttc", "msjh.ttf", "mingliu.ttc"];
    /// Segoe UI has Hebrew and Arabic, and Armenian, Georgian and more.
    const GENERAL: &[&str] = &["segoeui.ttf", "tahoma.ttf", "arial.ttf", "seguisym.ttf"];
    const THAI: &[&str] = &["LeelawUI.ttf", "leelawad.ttf"];
    const INDIC: &[&str] = &["Nirmala.ttc", "Nirmala.ttf"];

    impl System {
        pub(crate) fn new() -> System {
            let root = ["SystemRoot", "windir"]
                .iter()
                .find_map(|name| std::env::var_os(name).filter(|value| !value.is_empty()))
                .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
            System {
                folder: root.join("Fonts"),
                user: user_language(),
            }
        }
    }

    /// The language Windows shows its own texts in, if it is one of these.
    fn user_language() -> Option<Cjk> {
        // SAFETY: takes no arguments and returns a number.
        let language = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
        cjk_of_language(language)
    }

    /// A Windows language identifier: the language in its low ten bits, the
    /// variant above them.
    pub(crate) fn cjk_of_language(language: u16) -> Option<Cjk> {
        match (language & 0x3FF, language >> 10) {
            (0x11, _) => Some(Cjk::Japanese),
            (0x12, _) => Some(Cjk::Korean),
            // Taiwan, Hong Kong and Macao write the traditional characters.
            (0x04, 0x01 | 0x03 | 0x05) => Some(Cjk::Traditional),
            (0x04, _) => Some(Cjk::Simplified),
            _ => None,
        }
    }

    impl Finder for System {
        fn user(&self) -> Option<Cjk> {
            self.user
        }

        fn candidates(&self, letter: char, cjk: Option<Cjk>) -> Vec<FontFile> {
            let lists: Vec<&[&str]> = match (super::script(letter), cjk) {
                (Script::Thai, _) => vec![THAI, GENERAL],
                (Script::Indic, _) => vec![INDIC],
                // Each of the four has most of the others' characters.
                (_, Some(Cjk::Japanese)) => vec![JAPANESE, SIMPLIFIED, TRADITIONAL, KOREAN],
                (_, Some(Cjk::Korean)) => vec![KOREAN, SIMPLIFIED, TRADITIONAL, JAPANESE],
                (_, Some(Cjk::Simplified)) => vec![SIMPLIFIED, TRADITIONAL, JAPANESE, KOREAN],
                (_, Some(Cjk::Traditional)) => vec![TRADITIONAL, SIMPLIFIED, JAPANESE, KOREAN],
                (_, None) => vec![GENERAL],
            };
            lists
                .into_iter()
                .flatten()
                .map(|name| FontFile::new(self.folder.join(name)))
                .filter(|file| file.path.is_file())
                .collect()
        }
    }
}

#[cfg(not(windows))]
pub(crate) mod os {
    use super::{Cjk, Finder, FontFile, Script};
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    /// What fontconfig would choose, which is what the desktop's own
    /// programs show: asked through `fc-match`, which comes with fontconfig.
    /// Where that is not installed, Noto Sans CJK and DejaVu Sans are looked
    /// for where the distributions put them.
    pub(crate) struct System {
        user: Option<Cjk>,
    }

    /// How long `fc-match` is waited for. It answers in a hundredth of a
    /// second; it takes seconds when it first has to read every typeface.
    const PATIENCE: Duration = Duration::from_secs(20);

    const NOTO_CJK: &[&str] = &[
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-sans-cjk-fonts/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/NotoSansCJK-Regular.ttc",
    ];
    /// Has Hebrew and Arabic.
    const DEJAVU: &[&str] = &[
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/DejaVuSans.ttf",
    ];

    impl System {
        pub(crate) fn new() -> System {
            let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
                .iter()
                .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
                .unwrap_or_default();
            System {
                user: cjk_of_locale(&locale),
            }
        }
    }

    /// `ja_JP.UTF-8`, `zh_TW`, `ko_KR.eucKR` and the like.
    pub(crate) fn cjk_of_locale(locale: &str) -> Option<Cjk> {
        let name = locale.split(['.', '@']).next().unwrap_or_default();
        let (language, region) = name.split_once('_').unwrap_or((name, ""));
        match (language, region) {
            ("ja", _) => Some(Cjk::Japanese),
            ("ko", _) => Some(Cjk::Korean),
            ("zh", "TW" | "HK" | "MO") => Some(Cjk::Traditional),
            ("zh", _) => Some(Cjk::Simplified),
            _ => None,
        }
    }

    /// The face of Noto Sans CJK's collection for a language: Japanese,
    /// Korean, Simplified and Traditional Chinese, in that order.
    fn noto_face(cjk: Option<Cjk>) -> u32 {
        match cjk {
            Some(Cjk::Japanese) | None => 0,
            Some(Cjk::Korean) => 1,
            Some(Cjk::Simplified) => 2,
            Some(Cjk::Traditional) => 3,
        }
    }

    fn fontconfig_language(cjk: Cjk) -> &'static str {
        match cjk {
            Cjk::Japanese => "ja",
            Cjk::Korean => "ko",
            Cjk::Simplified => "zh-cn",
            Cjk::Traditional => "zh-tw",
        }
    }

    /// What `fc-match` is asked: the sans-serif face that has the letter,
    /// one that is drawn from outlines and in one colour (egui draws no
    /// others), for the language where a character is shared by several.
    pub(crate) fn pattern(letter: char, cjk: Option<Cjk>) -> String {
        let mut pattern = format!(
            "sans-serif:charset={:x}:scalable=true:color=false",
            letter as u32
        );
        if let Some(cjk) = cjk {
            pattern.push_str(":lang=");
            pattern.push_str(fontconfig_language(cjk));
        }
        pattern
    }

    /// The file and face in `answer`, which is `fc-match`'s to the format
    /// `%{file}\n%{index}\n`.
    pub(crate) fn parse(answer: &str) -> Option<FontFile> {
        let mut lines = answer.lines();
        let path = PathBuf::from(lines.next().filter(|line| line.starts_with('/'))?);
        // The face of a collection is in the low sixteen bits; above them
        // fontconfig keeps which variation of a variable face it means.
        let index = lines.next()?.trim().parse::<u32>().ok()? & 0xFFFF;
        Some(FontFile { path, index })
    }

    fn fc_match(mut program: Command, pattern: &str) -> Option<FontFile> {
        use std::io::Read as _;
        let mut child = program
            .args(["-f", "%{file}\n%{index}\n", pattern])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let deadline = Instant::now() + PATIENCE;
        let ended = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.success(),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break false;
                }
            }
        };
        let mut answer = String::new();
        // A few dozen bytes: they are in the pipe when the program has ended.
        child.stdout.take()?.read_to_string(&mut answer).ok()?;
        ended.then(|| parse(&answer)).flatten()
    }

    impl Finder for System {
        fn user(&self) -> Option<Cjk> {
            self.user
        }

        fn candidates(&self, letter: char, cjk: Option<Cjk>) -> Vec<FontFile> {
            let mut files = Vec::new();
            files.extend(fc_match(Command::new("fc-match"), &pattern(letter, cjk)));
            let (known, index): (&[&str], u32) = match super::script(letter) {
                Script::Han | Script::Kana | Script::Hangul | Script::Bopomofo => {
                    (NOTO_CJK, noto_face(cjk))
                }
                Script::Hebrew | Script::Arabic | Script::Other => (DEJAVU, 0),
                Script::Thai | Script::Indic => (&[], 0),
            };
            for path in known {
                let file = FontFile {
                    path: PathBuf::from(path),
                    index,
                };
                if file.path.is_file() && !files.iter().any(|f: &FontFile| f.path == file.path) {
                    files.push(file);
                }
            }
            files
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_locale_says_whose_chinese_characters_to_prefer() {
            assert_eq!(cjk_of_locale("ja_JP.UTF-8"), Some(Cjk::Japanese));
            assert_eq!(cjk_of_locale("ko_KR.eucKR"), Some(Cjk::Korean));
            assert_eq!(cjk_of_locale("zh_CN.UTF-8"), Some(Cjk::Simplified));
            assert_eq!(cjk_of_locale("zh_SG"), Some(Cjk::Simplified));
            assert_eq!(cjk_of_locale("zh_TW.UTF-8"), Some(Cjk::Traditional));
            assert_eq!(cjk_of_locale("zh_HK@euro"), Some(Cjk::Traditional));
            assert_eq!(cjk_of_locale("ja"), Some(Cjk::Japanese));
            for other in ["", "C", "POSIX", "en_US.UTF-8", "de_DE", "jam_JM", "zhx"] {
                assert_eq!(cjk_of_locale(other), None, "{other}");
            }
        }

        #[test]
        fn fontconfig_is_asked_for_the_letter_and_its_answer_is_read() {
            assert_eq!(
                pattern('写', Some(Cjk::Japanese)),
                "sans-serif:charset=5199:scalable=true:color=false:lang=ja"
            );
            assert_eq!(
                pattern('ש', None),
                "sans-serif:charset=5e9:scalable=true:color=false"
            );
            assert_eq!(
                parse("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc\n1\n"),
                Some(FontFile {
                    path: "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc".into(),
                    index: 1
                })
            );
            // A named variation of a variable face: 2 << 16, face 0.
            assert_eq!(
                parse("/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf\n131072\n"),
                Some(FontFile::new(
                    "/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf"
                ))
            );
            for wrong in ["", "\n0\n", "relative.ttf\n0\n", "/a.ttf\n", "/a.ttf\nx\n"] {
                assert_eq!(parse(wrong), None, "{wrong:?}");
            }
        }

        /// A stand-in `fc-match`, so that the answer does not depend on
        /// what is installed: a shell that is given the same arguments.
        fn stand_in(body: &str) -> Command {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", body, "fc-match"]);
            command
        }

        #[test]
        fn an_fc_match_that_fails_or_is_missing_gives_no_file() {
            // It is given the format and the pattern, and answers.
            let works = stand_in(
                r#"[ "$1" = -f ] && [ "$3" = "sans-serif:charset=5d0" ] && printf '/fonts/a b.ttc\n3\n'"#,
            );
            assert_eq!(
                fc_match(works, "sans-serif:charset=5d0"),
                Some(FontFile {
                    path: "/fonts/a b.ttc".into(),
                    index: 3
                })
            );
            let fails = stand_in(r"printf '/fonts/a.ttf\n0\n'; exit 1");
            assert_eq!(fc_match(fails, "x"), None);
            assert_eq!(fc_match(stand_in("exit 0"), "x"), None);
            let dir = tempfile::tempdir().unwrap();
            assert_eq!(
                fc_match(Command::new(dir.path().join("missing")), "x"),
                None
            );
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const FIRA: &[u8] = include_bytes!("../../../../assets/fonts/FiraSans-Regular.ttf");
    const BASKERVILLE: &[u8] =
        include_bytes!("../../../../tools/iconmaker/fonts/LibreBaskerville-Variable.ttf");

    /// A context whose only typeface is Libre Baskerville, which has no
    /// Greek and no Cyrillic: Fira Sans plays the system's typeface.
    fn latin_only() -> egui::Context {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::empty();
        fonts
            .font_data
            .insert("own".into(), Arc::new(FontData::from_static(BASKERVILLE)));
        for family in [
            FontFamily::Proportional,
            FontFamily::Monospace,
            super::super::theme::display_family(),
        ] {
            fonts.families.insert(family, vec!["own".into()]);
        }
        ctx.set_fonts(fonts);
        ctx
    }

    struct Fixed {
        files: Vec<FontFile>,
        asked: Arc<Mutex<Vec<char>>>,
    }

    impl Finder for Fixed {
        fn user(&self) -> Option<Cjk> {
            None
        }
        fn candidates(&self, letter: char, _: Option<Cjk>) -> Vec<FontFile> {
            self.asked.lock().unwrap().push(letter);
            self.files.clone()
        }
    }

    fn pass(ctx: &egui::Context, text: &str) {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label(text);
            });
        });
    }

    /// Draw `text` until nothing is looked for any more.
    pub(crate) fn draw_until_settled(fonts: &SystemFonts, mut draw: impl FnMut()) {
        let deadline = Instant::now() + Duration::from_secs(60);
        draw();
        while !fonts.settled() {
            assert!(Instant::now() < deadline, "the search did not end");
            std::thread::sleep(Duration::from_millis(2));
        }
        // The pass in which the added files take effect.
        draw();
    }

    fn shown(ctx: &egui::Context, text: &str) -> bool {
        ctx.fonts_mut(|fonts| fonts.has_glyphs(&FontId::proportional(14.0), text))
    }

    #[test]
    fn a_letter_the_built_in_faces_lack_is_drawn_with_a_file_of_the_system() {
        let dir = tempfile::tempdir().unwrap();
        // In the order they are tried: no file, a folder, an empty file,
        // one that is no typeface, half a typeface, one without the letter,
        // and the one that has it.
        let missing = FontFile::new(dir.path().join("missing.ttf"));
        let folder = FontFile::new(dir.path().join("folder.ttf"));
        std::fs::create_dir(&folder.path).unwrap();
        let empty = FontFile::new(dir.path().join("empty.ttf"));
        std::fs::write(&empty.path, b"").unwrap();
        let text = FontFile::new(dir.path().join("text.ttf"));
        std::fs::write(&text.path, b"this is not a typeface").unwrap();
        let cut = FontFile::new(dir.path().join("cut.ttf"));
        std::fs::write(&cut.path, &FIRA[..FIRA.len() / 2]).unwrap();
        let second_face = FontFile {
            path: dir.path().join("second-face.ttf"),
            index: 1,
        };
        std::fs::write(&second_face.path, FIRA).unwrap();
        let latin = FontFile::new(dir.path().join("latin.ttf"));
        std::fs::write(&latin.path, BASKERVILLE).unwrap();
        let greek = FontFile::new(dir.path().join("greek.ttf"));
        std::fs::write(&greek.path, FIRA).unwrap();
        let never = FontFile::new(dir.path().join("never.ttf"));
        std::fs::write(&never.path, FIRA).unwrap();

        let ctx = latin_only();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let logged = Arc::new(Mutex::new(Vec::new()));
        let fonts = install_with(
            &ctx,
            Fixed {
                files: vec![
                    missing,
                    folder,
                    empty,
                    text,
                    cut,
                    second_face,
                    latin,
                    greek.clone(),
                    never,
                ],
                asked: asked.clone(),
            },
            {
                let logged = logged.clone();
                move |line: &str| logged.lock().unwrap().push(line.to_owned())
            },
        );
        let title = "Ελληνικά and Русский";
        pass(&ctx, "Latin only, and Latin-1: déjà vu");
        assert!(fonts.settled());
        assert!(asked.lock().unwrap().is_empty(), "nothing was lacking");
        pass(&ctx, title);
        assert!(!shown(&ctx, title), "the built-in face has no Greek");
        draw_until_settled(&fonts, || pass(&ctx, title));
        assert!(shown(&ctx, title));
        assert_eq!(fonts.added(), vec![greek]);
        // One search for the Greek letters; the file that was added has the
        // Cyrillic ones too, so they were not looked for.
        assert_eq!(*asked.lock().unwrap(), vec!['Ε']);
        let logged = logged.lock().unwrap().clone();
        assert_eq!(logged.len(), 1, "{logged:?}");
        assert!(logged[0].contains("U+0395") && logged[0].contains("greek.ttf"));
        // Seen letters are not looked at again, whatever came of them.
        for _ in 0..3 {
            pass(&ctx, title);
        }
        assert!(fonts.settled());
        assert_eq!(asked.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_letter_no_file_has_is_looked_for_once_and_its_neighbours_not_at_all() {
        let dir = tempfile::tempdir().unwrap();
        let latin = FontFile::new(dir.path().join("latin.ttf"));
        std::fs::write(&latin.path, BASKERVILLE).unwrap();
        let ctx = latin_only();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let fonts = install_with(
            &ctx,
            Fixed {
                files: vec![latin.clone()],
                asked: asked.clone(),
            },
            |_: &str| {},
        );
        draw_until_settled(&fonts, || pass(&ctx, "αβγ"));
        assert!(!shown(&ctx, "αβγ"));
        assert!(fonts.added().is_empty());
        assert_eq!(*asked.lock().unwrap(), vec!['α', 'β', 'γ']);
        // The file was read for the first of them only.
        assert_eq!(
            fonts.state().refused.iter().cloned().collect::<Vec<_>>(),
            vec![(latin, 0x03)]
        );
        draw_until_settled(&fonts, || pass(&ctx, "αβγ"));
        assert_eq!(asked.lock().unwrap().len(), 3);
    }

    #[test]
    fn no_more_files_are_added_than_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let greek = FontFile::new(dir.path().join("greek.ttf"));
        std::fs::write(&greek.path, FIRA).unwrap();
        let ctx = latin_only();
        let fonts = install_with(
            &ctx,
            Fixed {
                files: vec![greek],
                asked: Arc::default(),
            },
            |_: &str| {},
        );
        for i in 0..MOST_FILES {
            let already = FontFile::new(format!("already-{i}.ttf"));
            fonts.state().added.push(already);
        }
        draw_until_settled(&fonts, || pass(&ctx, "α"));
        assert_eq!(fonts.added().len(), MOST_FILES);
        assert!(!shown(&ctx, "α"));
    }

    #[test]
    fn a_system_face_is_moved_onto_the_baseline_of_the_built_in_one() {
        let fira = line_of(&ab_glyph::FontRef::try_from_slice(FIRA).unwrap());
        assert_eq!(fira, own_line());
        let (ascent, height) = fira.unwrap();
        assert!((ascent - 0.935).abs() < 1e-3 && (height - 1.2).abs() < 1e-3);
        // The same face again lies where it is.
        assert_eq!(baseline_shift(fira, fira), 0.0);
        // Yu Gothic: ascent 0.88, descent 0.222, gap 0.5. Unmoved it stood
        // 0.256 of the size too high (3.6 points of a 14 point text).
        let shift = baseline_shift(fira, Some((0.880, 0.880 + 0.222 + 0.5)));
        assert!((shift - 0.256).abs() < 1e-3, "{shift}");
        // Microsoft YaHei: ascent 1.058, descent 0.262. A little too low.
        let shift = baseline_shift(fira, Some((1.058, 1.058 + 0.262)));
        assert!((shift + 0.063).abs() < 1e-3, "{shift}");
        // Numbers that make no sense, or none.
        assert_eq!(baseline_shift(fira, Some((9.0, 1.0))), 0.0);
        assert_eq!(baseline_shift(fira, Some((f32::NAN, 1.0))), 0.0);
        assert_eq!(baseline_shift(fira, None), 0.0);
        assert_eq!(baseline_shift(None, fira), 0.0);
    }

    #[test]
    fn signs_alone_do_not_send_for_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let greek = FontFile::new(dir.path().join("greek.ttf"));
        std::fs::write(&greek.path, FIRA).unwrap();
        let ctx = latin_only();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let fonts = install_with(
            &ctx,
            Fixed {
                files: vec![greek],
                asked: asked.clone(),
            },
            |_: &str| {},
        );
        // The title bar's square, arrows, a private-use sign, an emoji:
        // Libre Baskerville has none of them and Fira Sans has the arrows.
        let signs = "\u{25A1} \u{2192} \u{25B6} \u{E000} \u{1F600} \u{3001}";
        draw_until_settled(&fonts, || pass(&ctx, signs));
        assert!(!shown(&ctx, "\u{2192}"));
        assert!(asked.lock().unwrap().is_empty());
        assert!(fonts.added().is_empty());
        // With a letter beside them the file comes, and its signs with it.
        draw_until_settled(&fonts, || pass(&ctx, "\u{2192} \u{03A9}"));
        assert_eq!(*asked.lock().unwrap(), vec!['\u{03A9}']);
        assert!(shown(&ctx, "\u{2192}"));
    }

    #[test]
    fn scripts_and_whose_chinese_characters() {
        for (letter, expected) in [
            ('写', Script::Han),
            ('、', Script::Han),
            ('Ａ', Script::Han),
            ('モ', Script::Kana),
            ('ひ', Script::Kana),
            ('한', Script::Hangul),
            ('ㄅ', Script::Bopomofo),
            ('ש', Script::Hebrew),
            ('م', Script::Arabic),
            ('ก', Script::Thai),
            ('अ', Script::Indic),
            ('Ω', Script::Other),
            ('a', Script::Other),
        ] {
            assert_eq!(script(letter), expected, "{letter}");
        }
        // The user's language decides; without one, the letters beside.
        assert_eq!(cjk_of('写', &['写', '真'], None), Some(Cjk::Simplified));
        assert_eq!(cjk_of('写', &['写', 'の'], None), Some(Cjk::Japanese));
        assert_eq!(cjk_of('写', &['写', '한'], None), Some(Cjk::Korean));
        assert_eq!(
            cjk_of('写', &['写', 'の'], Some(Cjk::Traditional)),
            Some(Cjk::Traditional)
        );
        assert_eq!(cjk_of('の', &[], Some(Cjk::Korean)), Some(Cjk::Japanese));
        assert_eq!(cjk_of('한', &[], Some(Cjk::Japanese)), Some(Cjk::Korean));
        assert_eq!(cjk_of('ש', &['の'], Some(Cjk::Japanese)), None);
    }

    /// The file of this system that has `letter`, if one has.
    fn of_this_system(system: &os::System, letter: char, beside: &[char]) -> Option<FontFile> {
        let cjk = cjk_of(letter, beside, system.user());
        system.candidates(letter, cjk).into_iter().find(|file| {
            read(file).is_some_and(|bytes| {
                ab_glyph::FontRef::try_from_slice_and_index(&bytes, file.index)
                    .is_ok_and(|face| has(&face, letter))
            })
        })
    }

    /// The letters of `text` that `ctx` still draws as boxes although this
    /// system has a typeface for them. Letters the system has nothing for
    /// (a container or a server without typefaces) are not counted.
    pub(crate) fn undrawn(ctx: &egui::Context, text: &str) -> Vec<char> {
        let system = os::System::new();
        let letters: Vec<char> = text.chars().collect();
        letters
            .iter()
            .copied()
            .filter(|letter| !shown(ctx, &letter.to_string()))
            .filter(|letter| of_this_system(&system, *letter, &letters).is_some())
            .collect()
    }

    /// One letter of each script the system is asked for, with the file it
    /// names. A system that has no typeface for a script passes: what is
    /// asserted is that asking never panics, whatever is installed.
    #[test]
    fn this_system_can_be_asked_for_every_script() {
        let system = os::System::new();
        for letter in ['写', 'モ', '한', 'ㄅ', 'ש', 'م', 'ก', 'अ', 'Ω', '\u{E000}'] {
            let file = of_this_system(&system, letter, &[letter]);
            eprintln!(
                "U+{:04X} {:?}: drawn with {:?}",
                letter as u32,
                script(letter),
                file.map(|file| (file.path, file.index))
            );
        }
    }

    /// Windows 10 and 11 have these in every edition.
    #[cfg(windows)]
    #[test]
    fn windows_has_a_typeface_for_hebrew_and_arabic_and_the_language_ids_are_read() {
        use os::cjk_of_language;
        let system = os::System::new();
        for letter in ['ש', 'م'] {
            let files = system.candidates(letter, None);
            assert!(
                files.iter().any(|file| read(file).is_some_and(|bytes| {
                    ab_glyph::FontRef::try_from_slice_and_index(&bytes, file.index)
                        .is_ok_and(|face| has(&face, letter))
                })),
                "U+{:04X}: {files:?}",
                letter as u32
            );
        }
        assert_eq!(cjk_of_language(0x0411), Some(Cjk::Japanese));
        assert_eq!(cjk_of_language(0x0412), Some(Cjk::Korean));
        assert_eq!(cjk_of_language(0x0804), Some(Cjk::Simplified));
        assert_eq!(cjk_of_language(0x1004), Some(Cjk::Simplified));
        assert_eq!(cjk_of_language(0x0404), Some(Cjk::Traditional));
        assert_eq!(cjk_of_language(0x0C04), Some(Cjk::Traditional));
        assert_eq!(cjk_of_language(0x1404), Some(Cjk::Traditional));
        assert_eq!(cjk_of_language(0x0409), None);
        assert_eq!(cjk_of_language(0x0407), None);
    }
}
