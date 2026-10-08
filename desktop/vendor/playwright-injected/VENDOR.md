# Vendored: Playwright injected script (v1.63.0)

`injectedScriptSource.js` is Playwright's generated **injected script**, the
in-page half of every Playwright locator and of `ariaSnapshot()`, byte for
byte as Playwright itself ships it. It is used, unmodified, by the session
preview driver (`desktop/src-tauri/src/session_preview/driver/driver.js`),
which runs it in an isolated `WKContentWorld` of the preview webview.

| | |
|---|---|
| Upstream | https://github.com/microsoft/playwright, tag `v1.63.0` |
| Package | `playwright-core@1.63.0` from registry.npmjs.org |
| Tarball integrity | `sha512-rYCsBF/M5HjUch52bbtVONEFjv6Xu8sm8h72dNlR5bzIE1fvC/bxgspzkjSfU+MweEMmPM8KJebG6nnyxo5mCg==` |
| Tarball sha256 | `208593d4e1bcd8f8fe5f869cad1cc332dc7f1d70dc1d58c102dc3ac36e30f26c` |
| Source inside the package | `lib/coreBundle.js` (sha256 `549070af3acabb3efcc4f55bfe6210f9f7c2fcf633cf7eaa59bfe60719969171`), the string literal assigned to `source4` in the module `packages/playwright-core/src/generated/injectedScriptSource.ts` |
| This file's sha256 | `94103308b4f5791976b53543f5812be61ffb988574f7a51f412f87ab0ad60a85` |
| License | Apache-2.0 (`LICENSE`, `NOTICE`, copied from the same package) |

The literal is the esbuild bundle of `packages/injected/src/*` and
`packages/isomorphic/*` (aria snapshot, role/label/text/testid selector
engines, selector parser, YAML renderer). Its CSS tokenizer is Playwright's
port of Tab Atkins' `parse-css` (CC0), as noted in Playwright's own source.

## How it was extracted

```sh
npm pack playwright-core@1.63.0      # verify the integrity above
tar -xzf playwright-core-1.63.0.tgz
# Take the single-line `source4 = '…';` that follows the
# `injectedScriptSource.ts"() {` module header, evaluate only that string
# literal, and write the resulting string to injectedScriptSource.js.
```

No line of the file was changed. Playwright's own host evaluates it as

```js
(() => { const module = {}; /* this file */
  return new (module.exports.InjectedScript())(globalThis, options); })();
```

and the driver does the same. To update: repeat the steps for the new
version, update every hash and version in this file, and re-run the driver
tests (`cargo test --manifest-path desktop/src-tauri/Cargo.toml session_preview`).
