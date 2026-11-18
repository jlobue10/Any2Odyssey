# Files saved by earlier releases

One folder per release tag. Every file in it was written by that release's own code, none by hand, and is kept byte for byte (`.gitattributes` switches line-end conversion off here: the installation records have a byte-order mark and CR LF, the rest has neither).

`crates/core/tests/state_compat.rs` loads all of it with the current readers, `crates/app/src/ui/lens.rs` loads the lens files the way the viewer does, and `installer/tests/test-record-compat.ps1` reads the installation records the way the installer and the uninstaller do.

| Folder | What is in it | How it was made |
| --- | --- | --- |
| `default`, `all-changed`, `mode-window`, `max`, `min` | `settings.json`, `ui-settings.json` and, from 0.9.0, `lens.json` | A small program linked against that tag's `crates/core` saved the same settings through that tag's writers (`Preferences::save_to`, `UiSettings::save_to`, `fsutil::write_json_atomic` for the lens). |
| `status`, `status-ready` | `worker-status.json`, `desktop-capture-status.json`, `capture-border-status.json` | The same program, through that tag's `fsutil::write_json_atomic`, with a fixed time and process number. |
| `linux-gui` (0.9.0 on) | `ui-settings.json`, `lens.json`, and the `settings.json` that 0.9.0 to 0.9.2 wrote when they were closed | The published Linux program of that release, started under Xvfb with an empty home folder and driven with the pointer: Dark, the lens-interleave output, the three lens numbers dragged, the blue-green-red box ticked. |
| `windows-record/pwsh`, `windows-record/ps51` | `version.json` for a fresh full installation and for a controls-only upgrade from 0.8.0 | The two statements of that tag's `installer/install.ps1` that build and write the record, run under PowerShell 7 and under Windows PowerShell 5.1 with stand-in values (a profile folder with Latin, Cyrillic and CJK letters, an apostrophe and an ampersand; a fixed time). |

## Adding a release

After a release is tagged, add its folder so that the next change is compared with it:

```sh
A2O_WRITE_STATE_FIXTURES="$PWD/crates/core/tests/fixtures/releases/vX.Y.Z" \
  cargo test -p any2odyssey-core --test state_compat this_release_writes
```

writes the first two rows of the table with the current writers (run it on the tag). Copy `windows-record` from the release before and replace the four records with ones made by the tag's `install.ps1` (`installer/tests/test-record-compat.ps1` shows the two statements and the stand-in values), under both PowerShells. `linux-gui` is optional.

When `this_release_writes_what_the_newest_release_in_the_corpus_wrote` fails, the format of a saved file has changed. That can be right, but then an older release, which users go back to with `Install.cmd -AllowDowngrade` or the setup program's `/ALLOWDOWNGRADE`, reads the new file: keep the keys it looks for, with values of the same kind, and add the new release's folder in the same pull request.
