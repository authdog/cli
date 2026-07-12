import assert from "node:assert/strict"
import test from "node:test"
import worker from "./index.ts"

const env = {
  GITHUB_REPO: "authdog/cli",
  BIN_NAME: "authdog-cli",
}

test("POSIX installer supports old and new archive names", async () => {
  const response = await worker.fetch(
    new Request("https://cli.auth.dog/install"),
    env,
  )
  assert.equal(response.status, 200)
  const script = await response.text()
  assert.match(script, /SOURCE="\$TMP\/authdog"/)
  assert.match(script, /SOURCE="\$TMP\/authdog-cli"/)
  assert.match(script, /"\$DEST\/authdog"/)
  assert.match(script, /"\$DEST\/authdog-cli"/)
})

test("PowerShell installer supports old and new executable names", async () => {
  const response = await worker.fetch(
    new Request("https://cli.auth.dog/install.ps1"),
    env,
  )
  assert.equal(response.status, 200)
  const script = await response.text()
  assert.match(script, /"authdog\.exe"/)
  assert.match(script, /"authdog-cli\.exe"/)
  assert.match(script, /Compatibility command/)
})
