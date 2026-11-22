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
# file is checked against the SHA-256 below. Needs python3 with venv.
#
# usage: setup.sh [folder with the two wheels, to download into or use]
# Remove it again with: rm -r "<data folder>/depth-runtime" "<data folder>/depth-engines"
set -eu

ORT='onnxruntime-gpu==1.32.0 --hash=sha256:09637f3e38a1addc308dec5ef825a36eeb786e9615f45ec76aa29909df5cdd36'
TRT='tensorrt-cu13-libs==10.16.1.11 --hash=sha256:91142c8ab3c58bed213cf1a563a6eb4e4f0ac529d05b5f909073acece0e3b712'

die() { echo "depth-runtime: $*" >&2; exit 1; }

[ "$(uname -m)" = x86_64 ] || die "these libraries are for x86_64 only"
command -v python3 >/dev/null || die "python3 is needed"
data=${ANY2ODYSSEY_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/any2odyssey}
[ -d "$data" ] || die "no data folder $data: start Any2Odyssey once first"
for library in libcudart.so.13 libcublas.so.13 libcudnn.so.9; do
    ldconfig -p | grep -q "$library" || [ -e "/opt/cuda/lib64/$library" ] ||
        die "$library is not installed: this runtime needs CUDA 13 and cuDNN 9"
done

work=$(mktemp -d "$data/.depth-runtime.XXXXXX")
trap 'rm -rf "$work"' EXIT
trap 'exit 1' INT TERM HUP
wheels=${1:-$work/wheels}
mkdir -p "$wheels" "$work/new"

python3 -m venv "$work/venv" || die "python3 -m venv failed (Debian and Ubuntu: install python3-venv)"
printf '%s\n%s\n' "$ORT" "$TRT" >"$work/wanted.txt"
echo "depth-runtime: downloading (4 GB unless $wheels has them)..."
"$work/venv/bin/pip" download --quiet --disable-pip-version-check --no-deps --require-hashes \
    --only-binary :all: --python-version 3.12 --implementation cp --abi cp312 \
    --platform manylinux_2_28_x86_64 --extra-index-url https://pypi.nvidia.com \
    -d "$wheels" -r "$work/wanted.txt" || die "the download failed"

echo "depth-runtime: unpacking..."
python3 - "$wheels" "$work/new" "${1:-}" <<'PYTHON'
import hashlib, os, re, sys, tempfile, zipfile

wheels, out, external = sys.argv[1:]
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
for start, sha in wanted.items():
    names = [n for n in os.listdir(wheels) if n.startswith(start) and n.endswith(".whl")]
    if len(names) != 1:
        sys.exit(f"depth-runtime: expected one wheel {start}* in {wheels}, found {names}")
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
                sys.exit(f"depth-runtime: {path} is not the file this was written for (SHA-256 differs)")
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

# Publish the complete directory in one rename. On an upgrade the old
# installation moves into the staging directory for the exit trap to clean.
python3 - "$work/new" "$data/depth-runtime" <<'PYTHON'
import ctypes, errno, os, sys

new, live = sys.argv[1:]
try:
    os.rename(new, live)
except OSError as error:
    if error.errno not in (errno.ENOTEMPTY, errno.EEXIST):
        raise
    rename = ctypes.CDLL(None, use_errno=True).renameat2
    # Linux AT_FDCWD and RENAME_EXCHANGE, with both paths on the same mount.
    if rename(-100, os.fsencode(new), -100, os.fsencode(live), 2):
        code = ctypes.get_errno()
        raise OSError(code, os.strerror(code), live)
PYTHON
# Only invalidate the engine cache after the runtime has been installed.
rm -rf "$data/depth-engines"
echo "depth-runtime: done, in $data/depth-runtime ($(du -sh "$data/depth-runtime" | cut -f1))."
echo "Start Any2Odyssey again; a live screen's first half minute at each depth detail is spent making TensorRT's form of the model, once."
