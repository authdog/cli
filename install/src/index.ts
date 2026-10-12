/**
 * Install helper Worker: resolves Authdog CLI release assets and serves installer scripts.
 */

export interface Env {
  GITHUB_REPO: string;
  BIN_NAME: string;
}

/** Targets produced by `.github/workflows/release.yml`. */
const RELEASE_TARGETS = [
  'x86_64-unknown-linux-gnu',
  'aarch64-unknown-linux-gnu',
  'i686-unknown-linux-gnu',
  'armv7-unknown-linux-gnueabihf',
  'x86_64-unknown-linux-musl',
  'aarch64-unknown-linux-musl',
  'x86_64-apple-darwin',
  'aarch64-apple-darwin',
  'x86_64-pc-windows-msvc',
] as const;

type ReleaseTarget = (typeof RELEASE_TARGETS)[number];

interface GithubAsset {
  name: string;
  browser_download_url: string;
}

interface GithubRelease {
  tag_name: string;
  assets: GithubAsset[];
}

interface GithubTag {
  name: string;
}

function isReleaseTarget(s: string): s is ReleaseTarget {
  return (RELEASE_TARGETS as readonly string[]).includes(s);
}

function assetExtension(target: ReleaseTarget): string {
  return target.endsWith('windows-msvc') ? '.zip' : '.tar.gz';
}

function expectedAssetName(bin: string, tag: string, target: ReleaseTarget): string {
  return `${bin}-${tag}-${target}${assetExtension(target)}`;
}

/** Bare semver tags only (`0.1.0`, `0.1.0-beta.2`; optional leading `v`). */
function parseReleaseTag(tag: string): { major: number; minor: number; patch: number; pre: number } {
  const trimmed = tag.trim().replace(/^v/, '');
  const dash = trimmed.indexOf('-');
  const core = dash === -1 ? trimmed : trimmed.slice(0, dash);
  const suffix = dash === -1 ? '' : trimmed.slice(dash + 1);
  const parts = core.split('.').map((n) => parseInt(n, 10) || 0);
  const major = parts[0] ?? 0;
  const minor = parts[1] ?? 0;
  const patch = parts[2] ?? 0;
  let pre = Number.POSITIVE_INFINITY;
  if (suffix) {
    const beta = /^beta\.(\d+)$/.exec(suffix);
    pre = beta ? parseInt(beta[1], 10) : -1;
  }
  return { major, minor, patch, pre };
}

export function compareReleaseTags(a: string, b: string): number {
  const pa = parseReleaseTag(a);
  const pb = parseReleaseTag(b);
  if (pa.major !== pb.major) return pa.major - pb.major;
  if (pa.minor !== pb.minor) return pa.minor - pb.minor;
  if (pa.patch !== pb.patch) return pa.patch - pb.patch;
  return pa.pre - pb.pre;
}

function normalizeVersionParam(version: string | null): string | null {
  if (!version) return null;
  const v = version.trim();
  if (!v || v.toLowerCase() === 'latest') return null;
  return v;
}

const GITHUB_HEADERS: Record<string, string> = {
  Accept: 'application/vnd.github+json',
  'User-Agent': 'cli-install-worker',
  'X-GitHub-Api-Version': '2022-11-28',
};

async function fetchGithubRelease(repo: string, version: string | null): Promise<GithubRelease> {
  const headers = GITHUB_HEADERS;

  async function ghJson(path: string, cacheTtl: number): Promise<GithubRelease> {
    const url = `https://api.github.com/${path}`;
    const res = await fetch(url, {
      headers,
      cf: { cacheEverything: true, cacheTtl },
    });
    if (!res.ok) {
      const body = await res.text();
      throw new Error(`GitHub API ${res.status}: ${body.slice(0, 500)}`);
    }
    return res.json() as Promise<GithubRelease>;
  }

  async function ghJsonArray(path: string, cacheTtl: number): Promise<GithubRelease[]> {
    const url = `https://api.github.com/${path}`;
    const res = await fetch(url, {
      headers,
      cf: { cacheEverything: true, cacheTtl },
    });
    if (!res.ok) {
      const body = await res.text();
      throw new Error(`GitHub API ${res.status}: ${body.slice(0, 500)}`);
    }
    return res.json() as Promise<GithubRelease[]>;
  }

  async function fetchPublishedReleases(maxPages = 5): Promise<GithubRelease[]> {
    const out: GithubRelease[] = [];
    for (let page = 1; page <= maxPages; page++) {
      const batch = await ghJsonArray(`repos/${repo}/releases?per_page=100&page=${page}`, 120);
      if (batch.length === 0) break;
      for (const r of batch) {
        if (!(r as GithubRelease & { draft?: boolean }).draft) {
          out.push(r);
        }
      }
      if (batch.length < 100) break;
    }
    return out;
  }

  function pickNewestRelease(releases: GithubRelease[]): GithubRelease {
    if (releases.length === 0) {
      throw new Error('GitHub API: no published releases');
    }
    let best = releases[0];
    for (const r of releases) {
      if (compareReleaseTags(r.tag_name, best.tag_name) > 0) {
        best = r;
      }
    }
    return best;
  }

  const pinned = normalizeVersionParam(version);
  if (pinned) {
    return ghJson(`repos/${repo}/releases/tags/${encodeURIComponent(pinned)}`, 3600);
  }

  const published = await fetchPublishedReleases();
  return pickNewestRelease(published);
}

async function fetchNewestGitTag(repo: string): Promise<string | null> {
  const url = `https://api.github.com/repos/${repo}/tags?per_page=100`;
  const res = await fetch(url, {
    headers: GITHUB_HEADERS,
    cf: { cacheEverything: true, cacheTtl: 120 },
  });
  if (!res.ok) return null;
  const tags = (await res.json()) as GithubTag[];
  if (tags.length === 0) return null;
  let best = tags[0].name;
  for (const t of tags) {
    if (compareReleaseTags(t.name, best) > 0) best = t.name;
  }
  return best;
}

export function resolveAssetUrl(release: GithubRelease, bin: string, target: ReleaseTarget): string {
  const prefixes = bin === 'authdog' ? (['authdog', 'authdog-cli'] as const) : ([bin] as const);
  const tried: string[] = [];
  for (const prefix of prefixes) {
    const want = expectedAssetName(prefix, release.tag_name, target);
    tried.push(want);
    const hit = release.assets.find((a) => a.name === want);
    if (hit) return hit.browser_download_url;
  }
  const names = release.assets.map((a) => a.name).sort();
  throw new Error(
    `No asset ${tried.join(' or ')} on ${release.tag_name}. Known assets: ${names.join(', ') || '(none)'}`,
  );
}

function plain(text: string, status = 200): Response {
  return new Response(text, {
    status,
    headers: {
      'content-type': 'text/plain; charset=utf-8',
      'cache-control': 'public, max-age=300',
    },
  });
}

function json(obj: unknown, status = 200): Response {
  return new Response(JSON.stringify(obj, null, 2), {
    status,
    headers: {
      'content-type': 'application/json; charset=utf-8',
      'cache-control': 'no-store',
    },
  });
}

function usage(origin: string): Response {
  const lines = [
    'Authdog CLI install worker',
    '',
    'Linux / macOS (newest published GitHub Release by semver, including betas):',
    `  curl -fsSL ${origin}/install | bash`,
    '',
    'Pin a release tag (must have GitHub Release assets — tag alone is not enough):',
    `  curl -fsSL '${origin}/install?version=0.1.0-beta.2' | bash`,
    `  curl -fsSL ${origin}/install | AUTHDOG_CLI_VERSION=0.1.0-beta.2 bash`,
    '',
    'Windows (PowerShell):',
    `  iwr -useb ${origin}/install.ps1 | iex`,
    '',
    'Resolve download URL for a release target triple (see GitHub Release workflow):',
    `  curl -fsSL '${origin}/v1/binary-url?target=x86_64-unknown-linux-gnu'`,
    '',
    'Newest published release tag — installable binaries (plain text):',
    `  curl -fsSL '${origin}/v1/release-tag'`,
    '',
    'Newest git tag on the repo (may lack GitHub Release assets):',
    `  curl -fsSL '${origin}/v1/newest-tag'`,
    '',
    'After install:',
    '- Run `authdog` in a terminal for the interactive TUI (or `authdog tui`). Use `authdog --help` for the full command list.',
    '',
    'Notes:',
    '- Installer prompts on a TTY before download (use /dev/tty when piped from curl). Skip with AUTHDOG_CLI_YES=1.',
    '- Latest install uses the newest **published** GitHub Release, not bare git tags. If a newer tag exists without release assets, the installer warns.',
    '- `VAR=1 curl … | bash` does not pass VAR to bash; put AUTHDOG_CLI_VERSION before `bash`, use ?version=, or rely on auto latest.',
    '- Linux: AUTHDOG_CLI_USE_MUSL=1 before `bash` (same piping rule) for musl builds (x86_64 / aarch64).',
    '- Override install dir: INSTALL_DIR=/usr/local/bin (POSIX) or $env:INSTALL_DIR (PowerShell).',
    '',
  ];
  return plain(lines.join('\n'));
}

/** Escape double quotes for embedding in shell double-quoted strings. */
function shellEscapeDouble(s: string): string {
  return s.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
}

function installSh(origin: string, queryVersion: string | null): Response {
  const o = shellEscapeDouble(origin);
  const baked =
    queryVersion && normalizeVersionParam(queryVersion)
      ? shellEscapeDouble(normalizeVersionParam(queryVersion)!)
      : '';
  const bakedBlock = baked
    ? `if [ -z "$VERSION" ]; then
  VERSION="${baked}"
fi
`
    : '';
  const script = `#!/usr/bin/env bash
# Authdog CLI installer — downloads the GitHub Release binary for this OS/arch.
set -euo pipefail

BASE="${o}"
VERSION=""
if [ -n "\${AUTHDOG_CLI_VERSION:-}" ]; then
  VERSION="\${AUTHDOG_CLI_VERSION}"
fi
${bakedBlock}resolve_latest_version() {
  case "$VERSION" in
    ""|latest|Latest|LATEST)
      VERSION="$(curl -fsSL "$BASE/v1/release-tag")"
      ;;
  esac
}

is_pinned_version() {
  case "$VERSION" in
    ""|latest|Latest|LATEST) return 1 ;;
    *) return 0 ;;
  esac
}

confirm_install() {
  if [ "\${AUTHDOG_CLI_YES:-}" = "1" ] || [ "\${AUTHDOG_CLI_NON_INTERACTIVE:-}" = "1" ]; then
    return 0
  fi
  local reply
  local prompt="Proceed with install? [y/N] "
  if [ -t 0 ]; then
    printf '%s' "$prompt" >&2
    read -r reply
  elif [ -r /dev/tty ]; then
    printf '%s' "$prompt" >/dev/tty
    read -r reply </dev/tty
  else
    echo "error: non-interactive shell; set AUTHDOG_CLI_YES=1 to install without a prompt" >&2
    exit 1
  fi
  case "$reply" in
    y|Y|yes|Yes|YES) return 0 ;;
    *)
      echo "Install cancelled." >&2
      exit 0
      ;;
  esac
}

print_install_plan() {
  local dest="\${INSTALL_DIR:-$HOME/.local/bin}"
  local newest_tag
  newest_tag="$(curl -fsSL "$BASE/v1/newest-tag" 2>/dev/null || true)"
  echo "" >&2
  echo "Authdog CLI install" >&2
  echo "  Published release:  $VERSION" >&2
  if [ -n "$newest_tag" ] && [ "$newest_tag" != "$VERSION" ]; then
    echo "  Newer git tag:      $newest_tag (no installable GitHub Release yet — publish via Release workflow)" >&2
  fi
  if [ "$USE_LATEST" = 0 ]; then
    echo "  Channel:  pinned (may be older than latest published release)" >&2
  else
    echo "  Channel:  latest published (GitHub Release with binaries at install time)" >&2
  fi
  echo "  Platform: $TARGET" >&2
  echo "  Install:  $dest" >&2
  echo "" >&2
}

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os" in
    Linux)
      case "$arch" in
        x86_64)
          if [ "\${AUTHDOG_CLI_USE_MUSL:-}" = "1" ]; then
            echo x86_64-unknown-linux-musl
          else
            echo x86_64-unknown-linux-gnu
          fi
          ;;
        aarch64 | arm64)
          if [ "\${AUTHDOG_CLI_USE_MUSL:-}" = "1" ]; then
            echo aarch64-unknown-linux-musl
          else
            echo aarch64-unknown-linux-gnu
          fi
          ;;
        i686 | i386 | x86)
          echo i686-unknown-linux-gnu
          ;;
        armv7l)
          echo armv7-unknown-linux-gnueabihf
          ;;
        *)
          echo "Unsupported Linux machine: $arch" >&2
          exit 1
          ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        x86_64) echo x86_64-apple-darwin ;;
        arm64) echo aarch64-apple-darwin ;;
        *)
          echo "Unsupported macOS machine: $arch" >&2
          exit 1
          ;;
      esac
      ;;
    *)
      echo "Unsupported OS: $os (use Windows install.ps1)" >&2
      exit 1
      ;;
  esac
}

TARGET="$(detect_target)"
USE_LATEST=1
if is_pinned_version; then USE_LATEST=0; fi
resolve_latest_version
print_install_plan
confirm_install
QUERY="target=$(printf '%s' "$TARGET" | sed -e 's/+/%2B/g')"
if [ -n "$VERSION" ]; then
  QUERY="$QUERY&version=$(printf '%s' "$VERSION" | sed -e 's/+/%2B/g')"
fi

TMP="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP"
}
trap cleanup EXIT

URL="$(curl -fsSL "$BASE/v1/binary-url?$QUERY")"
ARCHIVE="$TMP/authdog-download"
curl -fsSL "$URL" -o "$ARCHIVE"

case "$URL" in
  *.zip)
    unzip -q -o "$ARCHIVE" -d "$TMP"
    ;;
  *.tar.gz)
    tar xzf "$ARCHIVE" -C "$TMP"
    ;;
  *)
    echo "Unexpected release archive URL (want .zip or .tar.gz): $URL" >&2
    exit 1
    ;;
esac

DEST="\${INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$DEST"
SOURCE="$TMP/authdog"
if [ ! -f "$SOURCE" ]; then
  SOURCE="$TMP/authdog-cli"
fi
if [ ! -f "$SOURCE" ]; then
  echo "Release archive does not contain authdog (or legacy authdog-cli) binary" >&2
  exit 1
fi
install -m 0755 "$SOURCE" "$DEST/authdog"

echo ""
echo "Installed authdog $VERSION → $DEST/authdog"
if [ "$USE_LATEST" = 1 ]; then
  echo "You installed the latest published release (resolved via $BASE/v1/release-tag)."
fi
echo "Run \`authdog\` in a terminal for the interactive UI, or \`authdog --help\` for commands."
case ":$PATH:" in
  *:"$DEST":*) ;;
  *)
    echo "Hint: add $DEST to PATH if the command is not found." >&2
    ;;
esac
`;

  return new Response(script, {
    headers: {
      'content-type': 'text/plain; charset=utf-8',
      'cache-control': 'public, max-age=300',
    },
  });
}

/** Escape for double-quoted PowerShell strings. */
function psEscapeDouble(s: string): string {
  return s.replace(/`/g, '``').replace(/\$/g, '`$').replace(/"/g, '`"');
}

function installPs1(origin: string, queryVersion: string | null): Response {
  const o = psEscapeDouble(origin);
  const baked = queryVersion && normalizeVersionParam(queryVersion) ? psEscapeDouble(queryVersion.trim()) : '';
  const script = `# Authdog CLI installer for Windows (x86_64 MSVC build).
$ErrorActionPreference = "Stop"
$Base = "${o}"

$version = $env:AUTHDOG_CLI_VERSION
if (-not $version -and "${baked}") { $version = "${baked}" }
$pinned = $false
if ($version -and $version -ne "latest") { $pinned = $true }
if (-not $version -or $version -eq "latest") {
  $version = (Invoke-WebRequest -Uri "$Base/v1/release-tag" -UseBasicParsing).Content.Trim()
  $pinned = $false
}
$target = "x86_64-pc-windows-msvc"
$dest = $env:INSTALL_DIR
if (-not $dest) {
  $dest = Join-Path $env:LOCALAPPDATA "Programs\\authdog"
}
Write-Host ""
$newestTag = try { (Invoke-WebRequest -Uri "$Base/v1/newest-tag" -UseBasicParsing).Content.Trim() } catch { "" }
Write-Host "Authdog CLI install"
Write-Host "  Published release:  $version"
if ($newestTag -and $newestTag -ne $version) {
  Write-Host "  Newer git tag:      $newestTag (no installable GitHub Release yet — publish via Release workflow)"
}
if ($pinned) {
  Write-Host "  Channel:  pinned (may be older than latest published release)"
} else {
  Write-Host "  Channel:  latest published (GitHub Release with binaries at install time)"
}
Write-Host "  Platform: $target"
Write-Host "  Install:  $dest"
Write-Host ""
if ($env:AUTHDOG_CLI_YES -ne "1" -and $env:AUTHDOG_CLI_NON_INTERACTIVE -ne "1") {
  $reply = Read-Host "Proceed with install? [y/N]"
  if ($reply -notmatch '(?i)^(y|yes)$') {
    Write-Host "Install cancelled."
    exit 0
  }
}
$query = "target=$target"
if ($version) { $query += "&version=$([uri]::EscapeDataString($version))" }

$url = (Invoke-WebRequest -Uri "$Base/v1/binary-url?$query" -UseBasicParsing).Content.Trim()
$tmp = Join-Path $env:TEMP ("authdog-" + [guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
  $zip = Join-Path $tmp "authdog.zip"
  Invoke-WebRequest -Uri $url -OutFile $zip
  Expand-Archive -Path $zip -DestinationPath $tmp -Force

  New-Item -ItemType Directory -Path $dest -Force | Out-Null
  $exe = Join-Path $tmp "authdog.exe"
  if (-not (Test-Path $exe)) {
    $exe = Join-Path $tmp "authdog-cli.exe"
  }
  if (-not (Test-Path $exe)) {
    throw "Release archive does not contain authdog.exe (or legacy authdog-cli.exe)"
  }
  $out = Join-Path $dest "authdog.exe"
  Copy-Item -Force $exe $out
  Write-Host ""
  Write-Host "Installed authdog $version → $out"
  if (-not $pinned) {
    Write-Host "You installed the latest published release (resolved via $Base/v1/release-tag)."
  }
  Write-Host "Run authdog in a terminal for the interactive UI, or authdog --help for commands."
  Write-Host "Add to PATH if needed: $dest"
} finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
`;

  return new Response(script, {
    headers: {
      'content-type': 'text/plain; charset=utf-8; profile=powershell',
      'cache-control': 'public, max-age=300',
    },
  });
}

async function releaseTag(env: Env): Promise<Response> {
  const repo = env.GITHUB_REPO.trim();
  try {
    const release = await fetchGithubRelease(repo, null);
    return plain(release.tag_name, 200);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return json({ error: msg }, 502);
  }
}

async function newestGitTag(env: Env): Promise<Response> {
  const repo = env.GITHUB_REPO.trim();
  try {
    const tag = await fetchNewestGitTag(repo);
    if (!tag) return json({ error: 'no git tags' }, 502);
    return plain(tag, 200);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return json({ error: msg }, 502);
  }
}

async function binaryUrl(req: Request, env: Env): Promise<Response> {
  const url = new URL(req.url);
  const targetRaw = url.searchParams.get('target');
  const version = normalizeVersionParam(url.searchParams.get('version'));

  if (!targetRaw || !isReleaseTarget(targetRaw)) {
    return json(
      {
        error: 'Invalid or missing target',
        allowed: [...RELEASE_TARGETS],
      },
      400,
    );
  }

  const repo = env.GITHUB_REPO.trim();
  const bin = env.BIN_NAME.trim();

  try {
    const release = await fetchGithubRelease(repo, version);
    const assetUrl = resolveAssetUrl(release, bin, targetRaw);
    return plain(assetUrl);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return json({ error: msg }, 502);
  }
}

export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    const url = new URL(req.url);
    const path = url.pathname.replace(/\/+$/, '') || '/';
    const origin = url.origin;

    if (req.method !== 'GET' && req.method !== 'HEAD') {
      return new Response('Method Not Allowed', { status: 405 });
    }

    switch (path) {
      case '/':
        return usage(origin);
      case '/health':
        return plain('ok');
      case '/install':
      case '/install.sh':
        return installSh(origin, url.searchParams.get('version'));
      case '/install.ps1':
        return installPs1(origin, url.searchParams.get('version'));
      case '/v1/release-tag':
        return releaseTag(env);
      case '/v1/newest-tag':
        return newestGitTag(env);
      case '/v1/binary-url':
        return binaryUrl(req, env);
      default:
        return plain('Not found', 404);
    }
  },
};
