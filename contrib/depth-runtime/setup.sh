#!/bin/sh
# Put an ONNX Runtime with the TensorRT provider, and TensorRT, into
# Any2Odyssey's data folder (depth-runtime), where the program uses it before
# the system's: a live screen's depth maps are then made by TensorRT in half
# precision, several times as fast as by the CUDA provider.
#
# For an NVIDIA GPU on x86_64 with CUDA 13 and cuDNN 9 installed for the
# system (Arch: cuda, cudnn). Downloads 4 GB from PyPI (Microsoft's
# onnxruntime-gpu, MIT) and pypi.nvidia.com (NVIDIA's TensorRT, under NVIDIA's
# licence, which you accept by downloading it) and keeps 2.8 GB of it. Each
# file is checked against the SHA-256 below. Needs python3 (3.8 or newer) with
# venv, and 8 GB free in the data folder's file system while it runs.
#
# usage: setup.sh [folder with the two wheels, to download into or use]
# With both wheels whole in that folder nothing is downloaded: no pip, no venv
# and no network are needed then, and 7 GB free are enough.
# Remove it again with: rm -r "<data folder>/depth-runtime" "<data folder>/depth-engines"
set -eu

ORT='onnxruntime-gpu==1.32.0 --hash=sha256:09637f3e38a1addc308dec5ef825a36eeb786e9615f45ec76aa29909df5cdd36'
TRT='tensorrt-cu13-libs==10.16.1.11 --hash=sha256:91142c8ab3c58bed213cf1a563a6eb4e4f0ac529d05b5f909073acece0e3b712'

die() { echo "depth-runtime: $*" >&2; exit 1; }

[ "$(uname -m)" = x86_64 ] || die "these libraries are for x86_64 only"
command -v python3 >/dev/null || die "python3 is needed"
# An XDG_DATA_HOME that is not an absolute path counts as unset, as the
# program reads it (the XDG Base Directory specification).
case ${XDG_DATA_HOME:-} in /*) data_home=$XDG_DATA_HOME ;; *) data_home=$HOME/.local/share ;; esac
data=${ANY2ODYSSEY_DATA_DIR:-$data_home/any2odyssey}
if [ ! -d "$data" ]; then
    [ "$(id -u)" != 0 ] || die "no data folder $data (this runs as root: run it as yourself, without sudo)"
    die "no data folder $data: start Any2Odyssey once first"
fi
# As root by mistake, with the user's home kept (sudo -E): what root puts
# into a user's data folder, that user cannot replace again.
if [ "$(id -u)" = 0 ] && [ "$(ls -ldn "$data/." | awk 'NR == 1 { print $3 }')" != 0 ]; then
    die "this runs as root, and $data is another user's: run it as yourself, without sudo"
fi
# Said before anything is downloaded: the new runtime takes the old one's
# place by a rename, which a link (to another disk, say) cannot take part in.
if [ -L "$data/depth-runtime" ] || { [ -e "$data/depth-runtime" ] && [ ! -d "$data/depth-runtime" ]; }; then
    die "$data/depth-runtime is a link or a file, not a folder of its own: move it away and run this again"
fi
# Probe the exact SONAME with the loader used by the application. This
# includes LD_LIBRARY_PATH and does not require ldconfig on the user's PATH.
# libcurand is what the runtime's CUDA provider needs beside the others.
status=0
python3 - <<'PYTHON' || status=$?
import sys

if sys.version_info < (3, 8):
    sys.exit("depth-runtime: python3 is %d.%d here, and 3.8 or newer is needed" % sys.version_info[:2])
try:
    import ctypes
except ImportError as error:
    sys.exit(f"depth-runtime: this python3 ({sys.executable}) cannot open libraries: {error}. "
             "Use a whole one (Debian and Ubuntu: install python3; or put the system's first on the PATH).")

for library in ("libcudart.so.13", "libcublas.so.13", "libcurand.so.10", "libcudnn.so.9"):
    try:
        ctypes.CDLL(library)
    except OSError as error:
        sys.exit(f"depth-runtime: could not load {library}: {error}. "
                 "Install CUDA 13 and cuDNN 9 and make their libraries visible to the loader.")
PYTHON
# The loader ends the whole process over a library that is damaged (one that
# was cut short: "Bus error"), where no message can be made.
[ "$status" -lt 128 ] || die "python3 was ended by signal $((status - 128)) while it opened the libraries of CUDA 13 and cuDNN 9 (libcudart.so.13, libcublas.so.13, libcurand.so.10, libcudnn.so.9): one of them is damaged, as an interrupted installation leaves it. Install them again."
[ "$status" = 0 ] || exit 1

# Room while it runs: pip keeps a wheel beside the copy it delivers (twice
# 3.7 GB), and the wheels stay until what is kept of them is unpacked.
need=8
[ -z "${1:-}" ] || need=7
free=$(df -Pk "$data" 2>/dev/null | awk 'NR == 2 { print $4 }')
case $free in
'' | *[!0-9]*) ;;
*)
    [ "$free" -ge $((need * 1048576)) ] ||
        die "$data has $((free / 1048576)) GB free, and this needs $need GB there while it runs (2.8 GB afterwards)"
    ;;
esac

# What an earlier run left when it was ended outright (kill -9, a power cut):
# gigabytes that nothing shows. Each run notes its process in its folder; a
# folder without the note (1.0.5's and 1.0.6's script) goes when it is days old.
for stale in "$data"/.depth-runtime.*; do
    if [ -f "$stale/pid" ]; then
        ! kill -0 "$(cat "$stale/pid")" 2>/dev/null || continue
    else
        [ -n "$(find "$stale" -prune -mtime +1 2>/dev/null)" ] || continue
    fi
    rm -rf "$stale" || echo "depth-runtime: could not remove $stale, which an earlier run left" >&2
done

work=$(mktemp -d "$data/.depth-runtime.XXXXXX")
trap 'rm -rf "$work"' EXIT
# PIPE and QUIT as well: dash runs no EXIT trap for a signal it does not catch.
trap 'exit 1' INT TERM HUP PIPE QUIT
echo $$ >"$work/pid"
wheels=${1:-$work/wheels}
mkdir -p "$wheels"

# Unpack what is kept of the two wheels in $wheels into $work/new, each wheel
# checked against its SHA-256. "unpack try" ends with status 3 and no message
# where a wheel is not there or is another file.
unpack() {
    rm -rf "$work/new"
    mkdir "$work/new"
    python3 - "$wheels" "$work/new" "$external" "${1:-}" <<'PYTHON'
import hashlib, os, re, sys, tempfile, zipfile

wheels, out, external, attempt = sys.argv[1:]
wanted = {
    "onnxruntime_gpu-1.32.0-": "09637f3e38a1addc308dec5ef825a36eeb786e9615f45ec76aa29909df5cdd36",
    "tensorrt_cu13_libs-10.16.1.11-": "91142c8ab3c58bed213cf1a563a6eb4e4f0ac529d05b5f909073acece0e3b712",
}
# The runtime with its providers, and of TensorRT what Linux uses.
keep = re.compile(
    r"^(onnxruntime/capi/libonnxruntime[^/]*\.so[.0-9]*"
    r"|tensorrt_libs/libnv(infer|onnxparser|infer_plugin)\.so\.\d+"
    r"|tensorrt_libs/libnvinfer_builder_resource_(?!win_)[^/]+)$"
)


def unusable(message):
    sys.exit(3 if attempt else f"depth-runtime: {message}")


for start, sha in wanted.items():
    names = [n for n in os.listdir(wheels) if n.startswith(start) and n.endswith(".whl")]
    if len(names) > 1:
        sys.exit(f"depth-runtime: expected one wheel {start}* in {wheels}, found {names}")
    if not names:
        unusable(f"expected one wheel {start}* in {wheels}, found {names}")
    path = os.path.join(wheels, names[0])
    # A caller-supplied wheel folder can change independently. Unpack the
    # exact bytes hashed, keeping a private snapshot only for external wheels.
    with open(path, "rb") as source:
        with (tempfile.TemporaryFile(dir=out) if external else source) as verified:
            digest = hashlib.sha256()
            while block := source.read(1 << 22):
                digest.update(block)
                if external:
                    verified.write(block)
            if digest.hexdigest() != sha:
                unusable(f"{path} is not the file this was written for (SHA-256 differs)")
            verified.seek(0)
            with zipfile.ZipFile(verified) as wheel:
                for member in wheel.infolist():
                    if not keep.match(member.filename):
                        continue
                    target = os.path.join(out, os.path.basename(member.filename))
                    with wheel.open(member) as member_file, open(target, "wb") as file:
                        while block := member_file.read(1 << 22):
                            file.write(block)
                    os.chmod(target, 0o755)
runtime = [n for n in os.listdir(out) if re.fullmatch(r"libonnxruntime\.so\.1\.\d+\.\d+", n)]
if len(runtime) != 1 or not os.path.exists(os.path.join(out, "libnvinfer.so.10")):
    sys.exit("depth-runtime: the wheels did not hold what was expected")
os.symlink(runtime[0], os.path.join(out, "libonnxruntime.so.1"))
PYTHON
}

external=${1:-}
# Both wheels whole in the folder named: no pip, no venv, no network.
status=3
if [ -n "$external" ]; then
    echo "depth-runtime: looking for the two wheels in $wheels..."
    status=0
    unpack try || status=$?
fi
case $status in
0) ;;
3)
    python3 -m venv "$work/venv" || die "python3 -m venv failed (Debian and Ubuntu: install python3-venv)"
    printf '%s\n%s\n' "$ORT" "$TRT" >"$work/wanted.txt"
    echo "depth-runtime: downloading (4 GB unless $wheels has them)..."
    # pip keeps a wheel in TMPDIR while it downloads it. /tmp is memory on
    # most desktops, and smaller than the larger wheel on some.
    mkdir "$work/tmp"
    TMPDIR="$work/tmp" "$work/venv/bin/pip" download --quiet --disable-pip-version-check --no-deps --require-hashes \
        --only-binary :all: --python-version 3.12 --implementation cp --abi cp312 \
        --platform manylinux_2_28_x86_64 --extra-index-url https://pypi.nvidia.com \
        -d "$wheels" -r "$work/wanted.txt" || die "the download failed"
    rm -rf "$work/tmp" "$work/venv"

    echo "depth-runtime: unpacking..."
    unpack
    ;;
*) exit 1 ;;
esac

# Publish the complete directory in one rename. On an upgrade the old
# installation moves into the staging directory for the exit trap to clean.
python3 - "$work/new" "$data/depth-runtime" "$work/old" <<'PYTHON'
import ctypes, errno, os, sys

new, live, old = sys.argv[1:]
try:
    os.rename(new, live)
except NotADirectoryError:
    sys.exit(f"depth-runtime: {live} is a link or a file, not a folder of its own. Move it away and run this again.")
except OSError as error:
    if error.errno not in (errno.ENOTEMPTY, errno.EEXIST):
        raise
    code = errno.ENOSYS
    rename = getattr(ctypes.CDLL(None, use_errno=True), "renameat2", None)
    # Linux AT_FDCWD and RENAME_EXCHANGE, with both paths on the same mount.
    if rename is not None and rename(-100, os.fsencode(new), -100, os.fsencode(live), 2) == 0:
        code = 0
    elif rename is not None:
        code = ctypes.get_errno()
    if code in (errno.EINVAL, errno.ENOSYS, errno.EOPNOTSUPP):
        # A kernel or a file system (NFS among them) that cannot exchange
        # two folders: the old one moves out first, and for a moment the
        # data folder has no runtime.
        os.rename(live, old)
        try:
            os.rename(new, live)
        except OSError:
            os.rename(old, live)
            raise
        print("depth-runtime: this file system cannot exchange two folders in one step; the old runtime was moved away first.")
    elif code:
        raise OSError(code, os.strerror(code), live)
PYTHON
# Only invalidate the engine cache after the runtime has been installed.
rm -rf "$data/depth-engines"
echo "depth-runtime: done, in $data/depth-runtime ($(du -sh "$data/depth-runtime" | cut -f1))."
echo "Start Any2Odyssey again; a live screen's first half minute at each depth detail is spent making TensorRT's form of the model, once."
