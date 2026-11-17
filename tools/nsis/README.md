# `tools/nsis/` — local NSIS toolchain cache

This directory holds a **gitignored, locally provisioned** copy of the NSIS
compiler so the pre-push hook can compile `installer/kenali.nsi` on the Linux
development host without root access. Nothing here is committed except this
README.

## Why

The installer script has repeatedly broken in CI in ways that a local compile
catches immediately: invalid macros (`Invalid command: "${GetFullPath}"`),
commands used from the wrong section, and `File` instructions pointing at
missing staged binaries. `scripts/validate-pre-push.sh` compiles the script
before every push.

## Layout

```
tools/nsis/
├── README.md                 (committed)
├── bin/makensis              (provisioned, gitignored)
├── share/nsis/Include/       (provisioned, gitignored)
├── share/nsis/Stubs/         (provisioned, gitignored)
└── lib/                      (provisioned, gitignored; only if .debs ship libs)
```

`makensis` locates its stubs and headers through `NSISDIR`, which must point at
the directory *containing* `Include/` and `Stubs/` — that is
`tools/nsis/share/nsis`.

## Provisioning (automatic)

You normally do not run anything: `scripts/validate-pre-push.sh` provisions the
toolchain on first use when `tools/nsis/` is empty. It:

1. runs `apt-get download nsis nsis-common` in a temporary directory — this
   needs **no root** and no `sudo`;
2. unpacks both `.deb` files with `dpkg-deb -x`;
3. copies `usr/bin/makensis` to `tools/nsis/bin/` and
   `usr/share/nsis/{Include,Stubs}` to `tools/nsis/share/nsis/`;
4. copies any shared libraries shipped by the packages to `tools/nsis/lib/`;
5. verifies the result with `makensis -VERSION`.

The cache is reused on every later run, with no network access.

## Manual provisioning

Same steps by hand, if you prefer to do it explicitly:

```sh
set -eu
mkdir -p tools/nsis/debs && cd tools/nsis/debs
apt-get download nsis nsis-common
mkdir -p ../extract
for d in ./*.deb; do dpkg-deb -x "$d" ../extract; done
cd .. && mkdir -p bin share/nsis
cp extract/usr/bin/makensis bin/makensis && chmod +x bin/makensis
cp -a extract/usr/share/nsis/Include share/nsis/
cp -a extract/usr/share/nsis/Stubs   share/nsis/
rm -rf debs extract
NSISDIR="$PWD/share/nsis" ./bin/makensis -VERSION
```

If `makensis -VERSION` fails with a missing shared object, copy the libraries
from the unpacked packages into `tools/nsis/lib/`; the validator prepends that
directory to `LD_LIBRARY_PATH` automatically. Distributions differ here: only
add what is actually missing rather than replacing system libraries wholesale.

## Offline behaviour

| State | Behaviour |
|---|---|
| `tools/nsis/` already populated | Compiles offline. No network access attempted. |
| Empty and network available | Provisioned once, then cached. |
| Empty and network unavailable | NSIS check is **skipped with a loud warning**; the push is not blocked. |

When a toolchain *is* reachable — vendored, provisioned, or `makensis` on
`PATH` — the compile always runs and any compile error fails the push. Skipping
is only for a missing toolchain, never for a broken script.

## Security note

These are unmodified binaries from the distribution's `nsis` packages, fetched
over the normal APT transport with its signature checking intact. Do not replace
them with hand-downloaded binaries. The cache is gitignored so third-party
binaries never enter the repository.
