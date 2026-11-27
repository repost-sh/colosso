---
title: "Updates"
---

A released Koyo looks for a newer release at launch, every hour after, and when the person chooses **Check for Updates…** in the Koyo menu. When one exists, a pill appears in the window's top bar, and one click downloads the release, checks it, and restarts Koyo into it. A scheduled job releases Koyo again whenever CEF carries a newer Chromium. This page says what the person sees, what a release consists of, what the app checks, and how releases are made. To set up what releases need, see [How to release Koyo](releasing.md).

## 1. What the person sees

The pill sits in the top bar beside the downloads button, in both layouts. It is absent while there is nothing to tell.

| Where the update stands | Pill | Tooltip, and Settings → About |
|---|---|---|
| A newer release is published | **Update Koyo** | Koyo 0.2.0 is available. Koyo downloads it, then restarts to install it. |
| It is downloading | A ring that fills, **Updating…** | Downloading Koyo 0.2.0: 120 of 254 MB. |
| It is being checked and copied | An empty ring, **Updating…** | Checking Koyo 0.2.0 and copying it beside this one. |
| It is staged, and the restart was called off | **Restart to Update** | Koyo 0.2.0 is ready. It installs when Koyo restarts or quits. |
| The install failed | **Retry Update**, in the danger colour | Couldn't update to Koyo 0.2.0, with why and what to do. |

One click on **Update Koyo** runs everything through to the restart. Before Koyo ends it asks about unsaved edits and about pages that ask before they are left, as a quit does. It does not ask about running terminals and chats and does not stop them: they live in their daemons, and the Koyo that opens picks them up. If the person calls the question off, the release stays staged and the pill reads **Restart to Update**. A staged release is put in place when Koyo quits, and Koyo opens again only after a restart. Logging out or shutting down ends the installer with everything else, so the release is offered again at the next launch.

Settings → About says where the update stands on its **Version** row, the quiet cases too: up to date and when it last looked, a look that failed and why, a release that needs a newer macOS, and a build that doesn't update itself. It has the same button as the pill and no other.

**Check for Updates…** in the Koyo menu looks now and shows Settings → About. The Version row reads "Checking for updates…" until the feed answers, then says the answer. A release already found, installing, or staged stands as it is. In a build that doesn't update itself the row says so. There is no setting for updates.

## 2. What is published

### 2.1 The files

One folder per architecture, served over HTTPS:

```text
<base>/aarch64-apple-darwin/feed.json          replaced at each release, last
<base>/aarch64-apple-darwin/Koyo-0.2.0.dmg     written once, never replaced
```

The image's name is `Koyo-<version>.dmg`, beside the feed. Both the app and the pipeline derive it from the version, so the feed carries no file name and no address.

### 2.2 The feed

```json
{
  "version": "0.2.0",
  "minimum_macos": "14.0",
  "bytes": 199876543
}
```

| Field | Meaning | `script/release` reads it from |
|---|---|---|
| `version` | The release's version, as dotted numbers. | The bundle's `CFBundleShortVersionString` |
| `minimum_macos` | The oldest macOS the release starts on. | The bundle's `LSMinimumSystemVersion` |
| `bytes` | The image's size, for the progress, and the most the app downloads. | The image |

No value is typed by hand. A Koyo ignores fields that it doesn't know, so fields are only ever added.

The feed has no hash and no signature. It decides only whether a release is offered. What gets installed is decided by the image (section 4).

### 2.3 The image

The image is a disk image whose root holds `Koyo.app` and a link to Applications. It is compressed with LZMA (`ULMO`). Measured on the 608 MB bundle of 2026-11-25:

| Format | Size | Time to make | Time to copy the app out |
|---|---|---|---|
| `ULFO` (LZFSE) | 254 MB | 13 s | 0.9 s |
| `ULMO` (LZMA) | 194 MB | 254 s | 5.9 s |

The image is signed by the identity that signed the app, under the app's identifier:

```sh
codesign --sign "<identity>" -i codes.koyo.desktop --timestamp Koyo-0.2.0.dmg
```

That makes the image satisfy the app's own designated requirement, which is what a Koyo holds it to. A released image is notarized and stapled as well, for the people who download it in a browser. The updater doesn't look at the notarization.

### 2.4 How a bundle names its feed

`script/bundle --feed <address>` writes the feed's full address into the bundle's `Info.plist` as `KoyoUpdateFeed`. The signature seals it. Only `script/release` passes `--feed`.

A bundle without the key never looks for an update and makes no request: `cargo run`, `script/dev-bundle`, and a plain `script/bundle`. About says "This build doesn't update itself: it was built here, not released."

The address is in every Koyo ever released, so it cannot change. A second architecture gets its own folder, and its bundles name that folder's feed.

## 3. The app

The code is the `updates` module of the `koyo` crate. One entity, `Updates`, owns one value, `Stage`:

```text
Current ──look──▶ Available ──click──▶ Installing ──staged──▶ Ready ──restart──▶ Restarting
                     ▲                     │                    │
                     └──────click──── Failed ◀──failed──────────┘ (the installer didn't start)
```

`NeedsMacos` stands beside `Available` for a release this Mac can't run. It has no pill.

- **A look** reads the feed, at most 64 KiB. It changes the stage only while nothing is being installed.
- **A click** starts an install from `Available` or `Failed`, and a restart from `Ready`. In any other stage it does nothing, so a second click does no harm.
- **An install** runs on a thread of its own. It makes a folder for this attempt beside the bundle, downloads the image into it, checks the image, copies the app out, and checks the copy (section 4).
- **A restart** asks what a quit asks first, starts the installer, and quits only if the installer started.

### 3.1 One folder per attempt

```text
<folder of the bundle>/.Koyo.app.update-<pid>-<nanos>/     mode 0700, locked
    image.dmg     the download, removed once the app is copied out
    image/        where the image is mounted meanwhile
    Koyo.app      the staged bundle
```

The folder is beside the bundle so that both are on one volume. Making it is the install's first write, so a place Koyo can't write fails here, before anything downloads. The failure says where and what to do: "Koyo can't replace itself in /Volumes/Koyo. Quit Koyo, move it to Applications and open it from there."

The process that made the folder holds an exclusive lock on it. The kernel releases the lock when that process ends, however it ends. At launch, Koyo removes every attempt folder beside its bundle that nobody holds, after detaching an image still mounted in it. A folder whose lock is held belongs to a running Koyo or to an installer at work, and stays.

### 3.2 The installer

The bundle is never replaced while Koyo runs: the browser engine starts its helpers from the bundle by path. Koyo starts its own executable as `koyo install-update --attempt <folder>` and then quits. For a restart it adds `--reopen`.

The installer does this:

1. It takes the attempt only if the folder is one of its own bundle's attempt folders, a sibling of the bundle named `.<bundle>.update-…`.
2. It waits for the folder's lock, which it gets once the Koyo that staged the release has ended.
3. It checks the staged bundle itself, as the app did (section 4). The bundle must satisfy the installer's own designated requirement, be newer than the installer, and start on this macOS.
4. It exchanges the staged bundle and the installed one in one step, with `renamex_np` and `RENAME_SWAP`. At every instant the installed path holds one whole bundle or the other.
5. It removes the attempt folder, which now holds the replaced bundle.
6. After a restart, it opens Koyo with `open -n`, and passes on every `KOYO_*` variable that it inherited.

The installer replaces only the bundle that its own executable runs from. It takes no path to another one. Any process can start `koyo install-update`, so the installer trusts nothing that it is told: a staged bundle that fails a check stays where it is, the installed bundle is as it was, and the log says why. After a restart, Koyo opens again on every path out of the installer.

One limit remains. Another process of the same user can write into the attempt folder between the installer's check and the exchange, because a folder that a user's process makes is open to that user's other processes.

### 3.3 What a crash leaves

| When | What is on disk | The next launch |
|---|---|---|
| While it downloads, checks, or copies | An attempt folder, perhaps with an image mounted in it | Removes the folder. The release is offered again. |
| While staged | An attempt folder with a whole copy | Removes the folder. The release is offered again and downloads again. |
| In the installer, before the exchange | The old bundle, whole, and the attempt folder | Removes the folder. |
| In the installer, after the exchange | The new bundle, whole. The old one is in the attempt folder. | Removes the folder. |

### 3.4 Not designed for

Two Koyos started from one bundle keep their attempts apart, since each has its own folder. If one of them installs an update, the other goes on running from a bundle that was exchanged under it, and would start engine helpers of the new release. Running two Koyos from one bundle is not supported.

## 4. What is checked, and where

| Check | Read from | Where |
|---|---|---|
| The image satisfies the running Koyo's designated requirement, and is whole | The image's signature, before the image is mounted | `signature.rs`, `Requirement::vouch` |
| The copy satisfies the same requirement, under strict validation, its nested code included | The copy's signature | `staged.rs`, `may_replace` |
| The copy is newer than the running Koyo | The copy's `CFBundleShortVersionString` | `staged.rs`, `may_replace` |
| The copy starts on this macOS | The copy's `LSMinimumSystemVersion` | `staged.rs`, `may_replace` |
| The copy is the release that was offered | The copy's `CFBundleShortVersionString` | `staged.rs`, `Staged::check` |

`may_replace` decides whether a bundle may replace this Koyo. The app calls it when it stages a release, and the installer calls it again before the exchange. The app also checks that the copy is the release that it offered.

Before the copy is checked, it is flushed to disk: each file and folder is synced, and the drive is then asked once to write out its cache. A power loss just after the exchange therefore cannot leave the installed path with files whose contents never arrived.

The requirement is the running process's own, read from the system with `SecCodeCopySelf` and `SecCodeCopyDesignatedRequirement`. A Developer ID build's names its team. A build signed with this Mac's identity names its certificate. No other key exists.

Someone who can write to the bucket can withhold a release, or offer a genuine Koyo release that is newer than the installed one. They cannot install an older Koyo, or anything that Koyo's identity didn't sign under Koyo's identifier. A feed whose `bytes` is over 8 GiB is refused as malformed, so a feed cannot fill the disk.

The copy's architecture is not checked. A bundle names the feed of its own architecture, so another architecture's image is not reachable.

The macOS check needs a true `LSMinimumSystemVersion`. `script/browser-engine` and `script/bundle` read it from the binaries' load commands and take the newest. On 2026-11-25 the CEF 156 framework said 13.0 and the app's own executable 11.0, while the bundle had said 12.0 by hand.

## 5. Releases

A release starts in one of two ways. A person pushes a version tag, or the CEF job makes the newest release again on a newer CEF (section 6). Nothing else releases: a merge into `main` doesn't, and the workflow has no button.

### 5.1 The scripts

| Script | What it does |
|---|---|
| `script/release-version <version>` | Writes the version into the checkout for the build. `--next <version>` prints the version after it. |
| `script/cef` | Builds the CEF framework from source at the pinned commit. Starts clean when the pin moved. |
| `script/release --feed <address> --sign <identity> --notary <profile>` | Builds the bundle naming that feed, makes and signs the image, has Apple notarize it, checks the image against the app's own requirement, and writes `feed.json`. |
| `script/publish bucket` or `script/publish github` | Publishes the image and the feed, in the bucket or as the files of a GitHub release on the version's tag (5.6). Refuses a version that is published already and one that doesn't come after the published one. |
| `script/changelog <tag>` | Writes the tag's changelog from its commits (5.3). |

`script/release` without `--sign` signs with this Mac's identity (`script/signing-identity`). Only Koyos built on this Mac take that release. The proof in section 7 runs on it.

### 5.2 Versions

A release's version is its tag's: `v0.2.0` is Koyo 0.2.0. The workflow writes that version into its own checkout before it builds, and commits it nowhere. The version in `koyo-rs/koyo/Cargo.toml` is the development build's and says nothing about releases.

Versions only rise. A Koyo offers a release only under a version after its own, and `script/publish` refuses a version that doesn't come after the published one. A tag has three numbers and nothing else, because the app compares versions as numbers.

A release that the CEF job makes takes the version after the release it is made from: after `v0.2.0` comes `v0.2.1`.

### 5.3 The changelog

Each release gets a GitHub release on its tag, and the notes are its changelog. `script/changelog` writes it from the commits that the tag has and the release before it hasn't. The release before a tag is the nearest version tag among its ancestors. A tag with none is the first release, and its notes say so.

Commits are read as the repository writes them, `type(scope)!: what it does`. They are listed under their type, newest first, each with its short hash:

```markdown
### Breaking changes

- **layout:** one layout, arranged by Tabs, Address bar and Changes and Files (6d2cb0f15)

### Features

- **sidebar:** the scrolling rows fade at the top and the bottom (f43593ec6)

### Fixes

- **browser:** a page opened again shows its site's icon at once, not a placeholder (9f905b644)
```

A commit marked `!` is listed under breaking changes and not under its type. The types that have a heading are `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`, `chore`, `style`, and `revert`. Every commit is listed: one of another type, or with no type, goes under other changes. Merge commits are left out.

The changelog of a CEF release is the one commit that moved the pin.

### 5.4 The workflow

`.github/workflows/release.yml` runs on the project's own Mac, in this order:

1. Check out the tag. For a CEF release, check out the release it starts from, move the pin, commit that, and tag the commit.
2. Write the tag's version into the checkout.
3. Make a keychain for this run, with the Developer ID identity and the notary's credentials.
4. Build the CEF framework.
5. Run `script/check`.
6. Make the release.
7. Publish it to the bucket.
8. For a CEF release, push its tag.
9. Publish it on GitHub: the release on the tag, with the changelog as its notes and the image and the feed as its files.
10. For a CEF release, move the pin on `main` too.
11. Remove the keychain.

Nothing is published before step 7. A person's release pushes nothing: the tag was there before the run. A CEF release keeps its commit and its tag on the build Mac until step 8, so a run that fails earlier leaves no trace on GitHub.

### 5.5 What the workflow reads

| Kind | Name | Value |
|---|---|---|
| Variable | `KOYO_RELEASE_BASE` | `https://<host>/<path>`, where Koyos read releases |
| Variable | `KOYO_BUCKET_URL` | The same place as the bucket's S3 API writes it |
| Variable | `KOYO_BUCKET_REGION` | The region to sign for. Left out, `auto`. |
| Secret | `APPLE_CERTIFICATE_P12` | The Developer ID certificate and key, base64 |
| Secret | `APPLE_CERTIFICATE_PASSWORD` | That file's password |
| Secret | `APPLE_SIGNING_IDENTITY` | `Developer ID Application: <Org> (<TEAMID>)` |
| Secret | `APPLE_API_KEY`, `APPLE_API_KEY_ID`, `APPLE_API_ISSUER` | An App Store Connect key, for notarizing |
| Secret | `APPLE_PROVISIONING_PROFILE` | The Developer ID profile with Apple's browser passkey entitlement, base64. Empty until Apple grants it, and releases are then made without it. |
| Secret | `KOYO_BUCKET_ACCESS_KEY_ID`, `KOYO_BUCKET_SECRET_ACCESS_KEY` | A key that reads and writes the bucket |

### 5.6 Releases on GitHub, until the workflow runs

The workflow needs the build Mac and the bucket (`docs/releasing.md`). Until both exist, a release is made on a developer's Mac and published as a GitHub release.

The feed's address is then `https://github.com/repost-sh/koyo/releases/latest/download/feed.json`. GitHub redirects that address to the file named `feed.json` of the newest release, and the image's address, `…/latest/download/Koyo-<version>.dmg`, to the image of the same release. The app follows both redirects, over HTTPS only.

`script/publish github` makes the release as a draft, attaches the image and the feed, and then publishes it, so the feed and its image appear together. The release's notes are its changelog (5.3).

A Koyo released this way reads GitHub for good. The workflow moves those Koyos to the bucket without anyone's help: every release it makes names the bucket's feed and is published on GitHub as well (5.4, step 9), so each of them installs the first such release and reads the bucket from then on.

## 6. CEF updates

The browser host pins the `cef` crate to one version. The crate's version names its CEF after the `+`, as in `156.7.0+156.0.31`.

`.github/workflows/cef.yml` runs four times a day on GitHub's own runner. It checks out the newest release's tag and runs `script/cef-update --check`, which reads crates.io's index and prints the CEF that the release is on and the newest one, or nothing when they are the same. A release of the crate that carries the same CEF moves nothing. Before the first release there is no tag, and the job does nothing.

When there is a newer CEF, the job calls the release workflow for that tag. The run moves the pin with `script/cef-update`, builds the framework from source for the new commit, and releases under the version after the tag's.

The release is the newest release with the pin moved, and nothing else. Work on `main` that nobody has tagged stays unreleased. Once the release is out, the run moves the pin on `main` the same way, with `script/cef-update`, so `main` only ever gets a pin that a release was made on, and the next tag that a person pushes carries it.

Two things stop a move, and each fails the run before anything is published:

- One of Koyo's patches (`script/cef-patches`) doesn't apply to the new Chromium.
- The host no longer builds against the new crate, or `script/check` fails.

The job tries again every six hours until someone fixes the cause.

## 7. Proof

Run on 2026-11-25, on this Mac, with everything inside the checkout. The feed was a local HTTPS server with a throwaway certificate, which the system `curl` accepts when `CURL_CA_BUNDLE` names it. No bucket, no Developer ID, and no GitHub runner were involved.

### 7.1 What ran

| What | How | Result |
|---|---|---|
| `script/release`, signed with this Mac's identity | `script/release` at version 0.1.1, its feed on `https://localhost:48443` | A 202 MB image and its `feed.json`. The image satisfies the built app's requirement. |
| `script/release-version` | `script/release-version 0.1.1`, then a build | The app and the bundle report 0.1.1. |
| `script/cef-update` | Against crates.io | It moved the pin and the lockfile from CEF 156.0.14 to 156.0.31, and printed nothing on a second run. |
| `script/changelog` | On a range of 447 commits | All 447 listed, grouped by type. |
| A release build finds its feed | `script/bundle --feed …`, run as its own instance | It logged "Koyo 0.1.0 at … reads https://localhost:48443/…" and "the feed offers Koyo 0.1.1". |
| The click-through, on a debug bundle with the probe | See 7.2 | Koyo 0.1.0 replaced itself with 0.1.1. |

### 7.2 The click-through

Two debug bundles, 0.1.0 and 0.1.1, were given a test identifier and this instance's folders, and signed with this Mac's identity. The 0.1.1 bundle went into an image signed under the same identifier. The 0.1.0 bundle ran on the agent desktop (the verify skill), driven through its probe.

1. With an image that has one byte flipped behind the feed, a click on the pill downloaded it. The app refused it: "invalid signature (code or signature have been modified)". No attempt folder was left.
2. With the good image, and an unsaved edit open, a second click downloaded it, logged "the image is signed as this Koyo is", and staged 0.1.1 beside the bundle in 12 s. The restart then asked about the unsaved edit. Called off with Escape, nothing was handed over, and the installed bundle was still 0.1.0.
3. With the edit saved, a plain quit handed the release to the installer. The installer waited for the app to end and logged "… is replaced with Koyo 0.1.1". The installed bundle then said 0.1.1 and passed `codesign --verify --strict --deep`. No attempt folder was left, and no Koyo opened.
4. Opened again, the bundle ran as 0.1.1 and logged "the feed has nothing newer".

### 7.3 What did not run

- **Pictures of the pill.** The Mac's screen was locked during the run, so no window drew. The steps above were read from the log, the files, and app control.
- **A restart that opens Koyo again.** The installer's `open -n` would put a window on the owner's desktop, which the verify skill forbids, and no test starts an app. The exchange before it is tested and was run for real; the opening is three lines in the installer's `main`.
- **A Developer ID signature, notarization, and macOS's App Management protection** in `/Applications`. This Mac has no Developer ID identity.
- **`script/publish`, and the workflows.** There is no bucket and no runner yet, and no release has been published on GitHub. The workflows pass `actionlint`.
- **`script/cef` starting clean for a new CEF.** It is `automate-git.py --force-clean`, CEF's own, and a run is hours.

