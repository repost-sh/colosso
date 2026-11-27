---
title: "How to release Koyo"
---

A release is made by the **Release** workflow, which starts when you push a version tag. It needs three things to exist first: a Mac that builds, Apple's credentials, and the bucket. This page sets those up, then shows how to release and what to do when a release fails. What a release consists of is in [Updates](updates.md).

## Set up the Mac that builds

GitHub's own macOS runners have 14 GB of storage. A release builds Chromium, which takes 90 GB, so releases run on a Mac of the project's.

The Mac needs:

- Apple silicon, and macOS with Xcode installed.
- Xcode's Metal toolchain: `xcodebuild -downloadComponent MetalToolchain`.
- 150 GB free. The Chromium sources and the build stay in `~/.cache/cef-build` between releases.
- An account that stays signed in to its desktop. `script/check` starts real pages, and they need a window server.

Register it as a runner:

1. On GitHub, open the repository's **Settings**, then **Actions**, then **Runners**, and click **New self-hosted runner**. Choose **macOS** and **ARM64**.
2. On the Mac, run the download and `config.sh` commands that page shows. Keep the default labels, which are `self-hosted`, `macOS`, and `ARM64`.
3. Install the runner as a service of the signed-in account:

	```sh
	./svc.sh install
	./svc.sh start
	```

The repository is public, so limit who can start a run on that Mac. In **Settings**, **Actions**, **General**, set **Require approval for all external contributors**. Neither workflow runs on a pull request.

## Give the workflow Apple's credentials

A Koyo takes an update only if the same identity signed it, so every release is signed with one Developer ID. macOS also keeps what the person allowed Koyo (the screen, the camera, the microphone) by that identity.

1. In your Apple Developer account, create a **Developer ID Application** certificate and install it in Keychain Access.
2. Export the certificate with its private key as a `.p12` file, with a password.
3. In App Store Connect, under **Users and Access**, **Integrations**, create a team API key with the **Developer** role. Download its `.p8` file. Note the key ID and the issuer ID.
4. Store them as secrets:

	```sh
	base64 -i DeveloperID.p12 | gh secret set APPLE_CERTIFICATE_P12 -R repost-sh/koyo
	gh secret set APPLE_CERTIFICATE_PASSWORD -R repost-sh/koyo
	gh secret set APPLE_SIGNING_IDENTITY -R repost-sh/koyo --body "Developer ID Application: <Org> (<TEAMID>)"
	gh secret set APPLE_API_KEY -R repost-sh/koyo < AuthKey_<KEYID>.p8
	gh secret set APPLE_API_KEY_ID -R repost-sh/koyo --body "<KEYID>"
	gh secret set APPLE_API_ISSUER -R repost-sh/koyo --body "<issuer id>"
	```

5. Delete the `.p12` and `.p8` files.

Keep the Developer ID for the life of the app. A Koyo signed by one team refuses a release signed by another, so a change of team strands every installed copy until its owner installs the new one by hand.

## Ask Apple for passkeys

Pages can use this Mac's passkeys only in a browser that Apple has granted its browser passkey entitlement. Until then, pages reach a security key or a phone. What the entitlement does in Koyo is in [Browser](browser.md), section 9.

Apple grants it only to a web browser, by criteria that [Browser](browser.md) section 9.4 lists against Koyo. Koyo's bundle declares the `http` and `https` schemes and opens a link that macOS hands it, which the criteria ask for. One criterion is not met and is yours to change: the Apple Developer account must be an organization's, and you its Account Holder. An individual membership does not qualify by Apple's wording.

To ask:

1. As the Account Holder, ask for the entitlement at <https://developer.apple.com/contact/request/macos-browsers-passkeys/>, for the App ID `codes.koyo.desktop`. Apple reviews the request, and that can take weeks.
2. In **Certificates, Identifiers & Profiles**, under **Identifiers**, create the explicit App ID `codes.koyo.desktop` if it doesn't exist. Once Apple has granted the entitlement, turn its capability on for that App ID.
3. Under **Profiles**, create a **Developer ID** provisioning profile for that App ID, with your Developer ID Application certificate. Download it as a `.provisionprofile` file.
4. Store it as a secret:

	```sh
	base64 -i Koyo.provisionprofile | gh secret set APPLE_PROVISIONING_PROFILE -R repost-sh/koyo
	```

The next release claims the entitlement. `script/bundle` refuses a profile that is for another App ID or that doesn't carry the entitlement, so a wrong file fails the release before it builds. A profile expires with its certificate: make a new one when you renew the certificate.

## Provision the bucket

The bucket, its host name `releases.koyo.codes`, and the release workflow's key to it are made by Terraform, in [distribution/](../distribution/README.md). Follow that page's setup once. It also stores `KOYO_RELEASE_BASE`, `KOYO_BUCKET_URL`, and the bucket's key in this repository, so there is nothing to set by hand.

## Release

To release a commit of `main`, tag it with the version and push the tag:

```sh
git tag v0.2.0 <commit>
git push origin v0.2.0
```

The tag is `v` and three numbers. The version must come after the published one.

The workflow builds that commit as Koyo 0.2.0, publishes it, and creates the GitHub release with the changelog since the release before. Write commit subjects as `type(scope): what it does`, because the changelog is made from them. Mark a breaking change with `!` before the colon.

Nothing else releases. A new CEF is the one exception: the **CEF** workflow releases the newest release again on it, by itself, four times a day.

## Release from your Mac, until the workflow runs

The workflow needs the build Mac and the bucket. Without them, make the release on a Mac that has the CEF framework built (`script/cef`) and the Developer ID in a keychain, and publish it on GitHub.

1. Store the notary's credentials once, from the App Store Connect key:

	```sh
	xcrun notarytool store-credentials koyo-notary --key AuthKey_<KEYID>.p8 --key-id <KEYID> --issuer <issuer id>
	```

2. Tag the commit and push the tag, as above.
3. Build it under the tag's version, signed and notarized:

	```sh
	script/release-version 0.2.0
	script/release --feed https://github.com/repost-sh/koyo/releases/latest/download/feed.json \
		--sign "Developer ID Application: <Org> (<TEAMID>)" --notary koyo-notary
	git checkout -- koyo-rs/koyo/Cargo.toml koyo-rs/Cargo.lock
	```

4. Publish it:

	```sh
	GITHUB_REPOSITORY=repost-sh/koyo GITHUB_TOKEN=$(gh auth token) script/publish github
	```

Koyos installed from such a release update themselves from GitHub. How they move to the bucket later is in [Updates](updates.md), section 5.6.

## When a release fails

Nothing is published until the **Publish to the bucket** step. A run that fails earlier leaves the published Koyo as it was.

| The step that failed | What it means | What to do |
|---|---|---|
| **Build the CEF framework**, in `script/cef-framework patch` | One of Koyo's patches doesn't apply to the new Chromium. The output names it. | Rebase the patch in `script/cef-patches`, merge it, and tag a release that carries it. |
| **Build the CEF framework**, in the build of `koyo-browser-host` | The `cef` crate's API changed. | Fix the host against the new crate, with the pin moved by `script/cef-update`, and tag a release. |
| **Check** | The repository's checks fail on what would be released. | Fix it, and push a tag with the next version. |
| **Make the release**, in `notarytool` | Apple refused the image. The output has the log's ID. | Read `xcrun notarytool log <id>`. |
| **Publish to the bucket**, after the image went up | The image is in the bucket and the feed doesn't name it. No Koyo sees it. | Push a tag with the next version. An image is never written twice. |
| **Publish on GitHub** | The release is out in the bucket, and GitHub has no release for the tag. Koyos that read the bucket are unaffected. | Run `script/publish github` by hand from the build Mac's checkout, as below. |
| **Move main to the released CEF** | The release is out, and `main` lacks its pin. | Run `script/cef-update` on `main` and commit the result. |

To run a failed release again without a change, open the run on GitHub and click **Re-run all jobs**. That works for a run that failed before **Publish to the bucket**.

Until someone fixes a failing CEF move, the **CEF** workflow tries it again every six hours. Each try after the first costs minutes, because the sources are already fetched.
