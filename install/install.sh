#!/usr/bin/env sh
# Installer for the Outlaw Repair Kit, by Outlaw Systems.
#
#   curl -fsSL https://raw.githubusercontent.com/Sup095/outlaw-repair-kit/main/install/install.sh | sh
#
# What it does, and nothing else:
#   * works out which release asset fits this machine
#   * downloads it, and refuses to install if the checksum does not match
#   * puts `outlaw` in ~/.local/bin
#   * if asked, sets up a local model sized to the graphics card it finds
#
# It never installs anything else without asking first, and it prints the
# exact command before running it. Run it with --help to see the options.

set -eu

REPO="Sup095/outlaw-repair-kit"
INSTALL_DIR="${OUTLAW_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="latest"
WITH_MODEL="ask"
WITH_DESKTOP="no"
ASSUME_YES="no"

say() { printf '%s\n' "$*"; }
step() { printf '\033[38;5;214m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[38;5;220mwarning:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[38;5;196merror:\033[0m %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<'USAGE'
Usage: install.sh [options]

  --version <tag>     Install a specific release (default: the newest)
  --dir <path>        Where to put the program (default: ~/.local/bin)
  --desktop           Also install the desktop app (an AppImage, no root needed)
  --local-model       Also set up a local model, without asking
  --no-local-model    Skip the local-model question entirely
  --yes               Do not ask anything; take the safe default each time
  --help              Show this
USAGE
}

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="${2:?--version needs a tag}"; shift 2 ;;
    --dir) INSTALL_DIR="${2:?--dir needs a path}"; shift 2 ;;
    --desktop) WITH_DESKTOP="yes"; shift ;;
    --local-model) WITH_MODEL="yes"; shift ;;
    --no-local-model) WITH_MODEL="no"; shift ;;
    --yes) ASSUME_YES="yes"; shift ;;
    --help|-h) usage; exit 0 ;;
    *) die "unknown option $1 (try --help)" ;;
  esac
done

ask() {
  # $1 is the question. Answers no unless the person says yes, and answers no
  # on its own when there is nobody there to ask.
  if [ "$ASSUME_YES" = "yes" ] || [ ! -t 0 ]; then
    return 1
  fi
  printf '%s [y/N] ' "$1"
  read -r reply || return 1
  case "$reply" in [yY]*) return 0 ;; *) return 1 ;; esac
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "this installer needs $1, which is not installed"
}

need curl
need tar
command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 ||
  die "this installer needs sha256sum or shasum to check what it downloaded"

checksum_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

case "$(uname -m)" in
  x86_64|amd64) ARCH="x86_64" ;;
  # Published builds are x86-64 only so far. Saying so beats a confusing
  # failure three steps later.
  *) die "no build is published for $(uname -m) yet -- build from source with: cargo install --git https://github.com/$REPO ork-cli" ;;
esac

case "$(uname -s)" in
  Linux) TARGET="${ARCH}-unknown-linux-gnu" ;;
  Darwin) die "macOS builds are not published yet -- see the README for building from source" ;;
  *) die "unsupported system: $(uname -s)" ;;
esac

step "Finding the release to install"
if [ "$VERSION" = "latest" ]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" |
    sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)
  [ -n "$VERSION" ] || die "could not work out the newest release -- try --version v0.4.0"
fi
say "  $VERSION for $TARGET"

ASSET="outlaw-${VERSION}-${TARGET}.tar.gz"
BASE="https://github.com/$REPO/releases/download/$VERSION"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT INT TERM

step "Downloading $ASSET"
curl -fSL --progress-bar "$BASE/$ASSET" -o "$WORK/$ASSET" ||
  die "could not download $ASSET -- check https://github.com/$REPO/releases"

step "Checking what was downloaded"
if curl -fsSL "$BASE/SHA256SUMS" -o "$WORK/SHA256SUMS" 2>/dev/null; then
  expected=$(grep " \*\{0,1\}$ASSET\$" "$WORK/SHA256SUMS" | cut -d' ' -f1 | head -n1)
  actual=$(checksum_of "$WORK/$ASSET")
  if [ -z "$expected" ]; then
    warn "that release publishes no checksum for $ASSET"
  elif [ "$expected" != "$actual" ]; then
    # Refusing is the only safe answer: a file that is not the published one
    # is not going anywhere near anybody's PATH.
    die "the download does not match its published checksum -- not installing it"
  else
    say "  checksum matches"
  fi
else
  warn "that release publishes no SHA256SUMS file, so the download could not be verified"
fi

step "Installing to $INSTALL_DIR"
mkdir -p "$INSTALL_DIR"
tar xzf "$WORK/$ASSET" -C "$WORK"
BIN=$(find "$WORK" -type f -name outlaw -perm -u+x | head -n1)
[ -n "$BIN" ] || die "the archive did not contain the program"
install -m 0755 "$BIN" "$INSTALL_DIR/outlaw"
say "  $INSTALL_DIR/outlaw"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    # Editing someone's shell profile without asking is not this script's
    # business, so it says what to add and leaves the file alone.
    warn "$INSTALL_DIR is not on your PATH. Add this to your shell profile:"
    say "    export PATH=\"\$PATH:$INSTALL_DIR\""
    ;;
esac

# --- optional: the desktop app ---------------------------------------------
#
# An AppImage into the same user-owned directory as the program. No package
# manager, no root: the same promise the rest of this script makes.

if [ "$WITH_DESKTOP" = "yes" ]; then
  step "Installing the desktop app"
  APPIMAGE="outlaw-repair-kit-${VERSION}-amd64.AppImage"
  if curl -fSL --progress-bar "$BASE/$APPIMAGE" -o "$WORK/$APPIMAGE"; then
    if [ -f "$WORK/SHA256SUMS" ]; then
      expected=$(grep " \*\{0,1\}$APPIMAGE\$" "$WORK/SHA256SUMS" | cut -d' ' -f1 | head -n1)
      actual=$(checksum_of "$WORK/$APPIMAGE")
      if [ -n "$expected" ] && [ "$expected" != "$actual" ]; then
        die "the desktop app does not match its published checksum -- not installing it"
      fi
    fi
    install -m 0755 "$WORK/$APPIMAGE" "$INSTALL_DIR/outlaw-repair-kit"
    say "  $INSTALL_DIR/outlaw-repair-kit"
    say "  run it with: outlaw-repair-kit"
    # An AppImage needs FUSE to mount itself. Said now rather than left as a
    # baffling failure the first time somebody double-clicks it.
    if ! command -v fusermount >/dev/null 2>&1 && ! command -v fusermount3 >/dev/null 2>&1; then
      warn "FUSE does not appear to be installed, which an AppImage needs. Install it, or run the app with --appimage-extract-and-run."
    fi
  else
    # Not fatal. The command-line program is installed and working.
    warn "no desktop app was published for $VERSION -- see https://github.com/$REPO/releases"
  fi
fi

# --- optional: a model on this machine -------------------------------------
#
# The tool works without any model at all: every deterministic check runs, and
# the runbook library explains known problems. A model only helps with the
# problems nobody has written down yet. So this is a question, not a step.

detect_vram_gb() {
  if command -v nvidia-smi >/dev/null 2>&1; then
    nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits 2>/dev/null |
      head -n1 | awk '{printf "%d", $1 / 1024}'
  elif command -v rocm-smi >/dev/null 2>&1; then
    rocm-smi --showmeminfo vram 2>/dev/null |
      awk '/Total/ {printf "%d", $NF / 1073741824; exit}'
  fi
}

model_for_vram() {
  # Sized so the model fits with room for its context, rather than the largest
  # one that technically loads.
  vram="${1:-0}"
  if [ "$vram" -ge 22 ]; then say "qwen3:32b"
  elif [ "$vram" -ge 14 ]; then say "qwen3:14b"
  elif [ "$vram" -ge 10 ]; then say "qwen3:8b"
  elif [ "$vram" -ge 6 ]; then say "qwen3:4b"
  else say "qwen3:1.7b"
  fi
}

if [ "$WITH_MODEL" = "ask" ]; then
  say ""
  say "The tool runs every check and explains known problems with no model at all."
  say "A model only helps with problems that are not in the runbook library."
  if ask "Set up a model on this machine as well?"; then WITH_MODEL="yes"; else WITH_MODEL="no"; fi
fi

if [ "$WITH_MODEL" = "yes" ]; then
  step "Setting up a local model"

  if ! command -v ollama >/dev/null 2>&1; then
    say "  Ollama is not installed. It is what runs the model."
    say "  The official installer would be run as:"
    say "      curl -fsSL https://ollama.com/install.sh | sh"
    if ask "  Run that now?"; then
      curl -fsSL https://ollama.com/install.sh | sh || die "the Ollama installer failed"
    else
      # Not a failure. The tool is installed and working; this part is extra.
      warn "skipping the model. Install Ollama or LM Studio later and the tool will find it."
      WITH_MODEL="no"
    fi
  fi
fi

if [ "$WITH_MODEL" = "yes" ]; then
  VRAM=$(detect_vram_gb || true)
  VRAM="${VRAM:-0}"
  MODEL=$(model_for_vram "$VRAM")
  if [ "$VRAM" -gt 0 ]; then
    say "  ${VRAM}GB of video memory found -- $MODEL fits comfortably"
  else
    say "  no graphics card found, so $MODEL was chosen to run on the processor"
  fi

  if ask "  Download $MODEL now? (several gigabytes)"; then
    ollama pull "$MODEL" || warn "could not download $MODEL -- run 'ollama pull $MODEL' yourself later"
  else
    say "  Skipped. Run 'ollama pull $MODEL' whenever you like."
  fi
fi

say ""
step "Done"
say "  outlaw boot      check everything is working"
say "  outlaw scan      look for problems"
say "  outlaw models    see which model would be used, and why"
say ""
say "  Made by Outlaw Systems, in collaboration with AI."
