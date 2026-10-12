import assert from "node:assert/strict"
import test from "node:test"
import worker, { compareReleaseTags, resolveAssetUrl } from "./index.ts"

const env = {
  GITHUB_REPO: "authdog/cli",
  BIN_NAME: "authdog",
}

test("semver release tag ordering", () => {
  assert.ok(compareReleaseTags("0.1.0-beta.2", "0.1.0-beta.1") > 0)
  assert.ok(compareReleaseTags("0.1.0", "0.1.0-beta.99") > 0)
  assert.equal(compareReleaseTags("0.1.0-beta.1", "0.1.0-beta.1"), 0)
})

test("resolveAssetUrl prefers authdog archives over legacy authdog-cli prefix", () => {
  const release = {
    tag_name: "0.1.0-beta.2",
    assets: [
      {
        name: "authdog-cli-0.1.0-beta.2-x86_64-unknown-linux-gnu.tar.gz",
        browser_download_url: "https://example.com/legacy.tar.gz",
      },
      {
        name: "authdog-0.1.0-beta.2-x86_64-unknown-linux-gnu.tar.gz",
        browser_download_url: "https://example.com/current.tar.gz",
      },
    ],
  }
  assert.equal(
    resolveAssetUrl(release, "authdog", "x86_64-unknown-linux-gnu"),
    "https://example.com/current.tar.gz",
  )
  const legacyOnly = {
    tag_name: "0.1.0-beta.1",
    assets: [
      {
        name: "authdog-cli-0.1.0-beta.1-x86_64-unknown-linux-gnu.tar.gz",
        browser_download_url: "https://example.com/legacy-only.tar.gz",
      },
    ],
  }
  assert.equal(
    resolveAssetUrl(legacyOnly, "authdog", "x86_64-unknown-linux-gnu"),
    "https://example.com/legacy-only.tar.gz",
  )
})

test("POSIX installer installs authdog binary only", async () => {
  const response = await worker.fetch(
    new Request("https://cli.auth.dog/install"),
    env,
  )
  assert.equal(response.status, 200)
  const script = await response.text()
  assert.match(script, /\/v1\/release-tag/)
  assert.match(script, /latest published/)
  assert.match(script, /v1\/newest-tag/)
  assert.match(script, /Proceed with install/)
  assert.match(script, /AUTHDOG_CLI_YES/)
  assert.match(script, /SOURCE="\$TMP\/authdog"/)
  assert.match(script, /SOURCE="\$TMP\/authdog-cli"/)
  assert.match(script, /"\$DEST\/authdog"/)
  assert.doesNotMatch(script, /"\$DEST\/authdog-cli"/)
  assert.match(script, /interactive UI/)
})

test("PowerShell installer installs authdog.exe only", async () => {
  const response = await worker.fetch(
    new Request("https://cli.auth.dog/install.ps1"),
    env,
  )
  assert.equal(response.status, 200)
  const script = await response.text()
  assert.match(script, /"authdog\.exe"/)
  assert.match(script, /"authdog-cli\.exe"/)
  assert.doesNotMatch(script, /Compatibility command/)
  assert.match(script, /Proceed with install/)
  assert.match(script, /latest published/)
})
