/**
 * Install helper Worker: resolves GitHub Release assets for `authdog-cli` and serves installer scripts.
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

function isReleaseTarget(s: string): s is ReleaseTarget {
  return (RELEASE_TARGETS as readonly string[]).includes(s);
}

function assetExtension(target: ReleaseTarget): string {
  return target.endsWith('windows-msvc') ? '.zip' : '.tar.gz';
}

function expectedAssetName(bin: string, tag: string, target: ReleaseTarget): string {
  return `${bin}-${tag}-${target}${assetExtension(target)}`;
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

  if (version) {
    return ghJson(`repos/${repo}/releases/tags/${encodeURIComponent(version)}`, 3600);
  }

  /** `/releases/latest` omits prereleases → 404 when only betas/rcs exist (see SPEC). */
  const latestUrl = `repos/${repo}/releases/latest`;
  const latestRes = await fetch(`https://api.github.com/${latestUrl}`, {
    headers,
    cf: { cacheEverything: true, cacheTtl: 300 },
  });
  if (latestRes.ok) {
    return latestRes.json() as Promise<GithubRelease>;
  }

  const latestBody = await latestRes.text();
  if (latestRes.status !== 404) {
    throw new Error(`GitHub API ${latestRes.status}: ${latestBody.slice(0, 500)}`);
  }

  const any = await ghJsonArray(`repos/${repo}/releases?per_page=1`, 300);
  const first = any[0];
  if (!first) {
    throw new Error(`GitHub API: no releases (latest returned 404: ${latestBody.slice(0, 200)})`);
  }
  return first;
}

function resolveAssetUrl(release: GithubRelease, bin: string, target: ReleaseTarget): string {
  const want = expectedAssetName(bin, release.tag_name, target);
  const hit = release.assets.find((a) => a.name === want);
  if (!hit) {
    const names = release.assets.map((a) => a.name).sort();
    throw new Error(`No asset ${want} on ${release.tag_name}. Known assets: ${names.join(', ') || '(none)'}`);
  }
  return hit.browser_download_url;
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
    'authdog-cli install worker',
    '',
    'Linux / macOS:',
    `  curl -fsSL ${origin}/install | bash`,
    '',
    'Windows (PowerShell):',
    `  iwr -useb ${origin}/install.ps1 | iex`,
    '',
    'Resolve download URL for a Rust triple (see GitHub Release workflow matrix):',
    `  curl -fsSL '${origin}/v1/binary-url?target=x86_64-unknown-linux-gnu'`,
    '',
    'Pin a version (bare semver tag, same as GitHub release tag — no leading v):',
    `  curl -fsSL '${origin}/v1/binary-url?target=aarch64-apple-darwin&version=0.1.0'`,
    '',
    'Notes:',
    '- Omitting version prefers the latest stable release; prereleases are used only when no stable exists yet.',
    '- Linux: set AUTHDOG_CLI_USE_MUSL=1 before piping install for static musl builds (x86_64 / aarch64 only).',
    '- Override install dir: INSTALL_DIR=/usr/local/bin (POSIX) or $env:INSTALL_DIR (PowerShell).',
    '',
  ];
  return plain(lines.join('\n'));
}

/** Escape double quotes for embedding in shell double-quoted strings. */
function shellEscapeDouble(s: string): string {
  return s.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
}

function installSh(origin: string): Response {
  const o = shellEscapeDouble(origin);
  const script = `#!/usr/bin/env bash
# Authdog CLI installer — downloads the GitHub Release binary for this OS/arch.
set -euo pipefail

BASE="${o}"
VERSION=""
if [ -n "\${AUTHDOG_CLI_VERSION:-}" ]; then
  VERSION="\${AUTHDOG_CLI_VERSION}"
fi

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
ARCHIVE="$TMP/authdog-cli-download"
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
install -m 0755 "$TMP/authdog-cli" "$DEST/authdog-cli"

echo "Installed authdog-cli → $DEST/authdog-cli"
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

function installPs1(origin: string): Response {
  const o = psEscapeDouble(origin);
  const script = `# Authdog CLI installer for Windows (x86_64 MSVC build).
$ErrorActionPreference = "Stop"
$Base = "${o}"

$version = $env:AUTHDOG_CLI_VERSION
$target = "x86_64-pc-windows-msvc"
$query = "target=$target"
if ($version) { $query += "&version=$([uri]::EscapeDataString($version))" }

$url = (Invoke-WebRequest -Uri "$Base/v1/binary-url?$query" -UseBasicParsing).Content.Trim()
$tmp = Join-Path $env:TEMP ("authdog-cli-" + [guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
  $zip = Join-Path $tmp "authdog-cli.zip"
  Invoke-WebRequest -Uri $url -OutFile $zip
  Expand-Archive -Path $zip -DestinationPath $tmp -Force

  $dest = $env:INSTALL_DIR
  if (-not $dest) {
    $dest = Join-Path $env:LOCALAPPDATA "Programs\\authdog-cli"
  }
  New-Item -ItemType Directory -Path $dest -Force | Out-Null
  $exe = Join-Path $tmp "authdog-cli.exe"
  $out = Join-Path $dest "authdog-cli.exe"
  Move-Item -Force $exe $out
  Write-Host "Installed authdog-cli → $out"
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

async function binaryUrl(req: Request, env: Env): Promise<Response> {
  const url = new URL(req.url);
  const targetRaw = url.searchParams.get('target');
  const version = url.searchParams.get('version');

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
        return installSh(origin);
      case '/install.ps1':
        return installPs1(origin);
      case '/v1/binary-url':
        return binaryUrl(req, env);
      default:
        return plain('Not found', 404);
    }
  },
};
