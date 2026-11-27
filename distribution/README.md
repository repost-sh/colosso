# Distribution

Terraform for what Koyo's releases are served from, in the Cloudflare account `Repost`:

| File | What it makes |
|---|---|
| `zone.tf` | The `koyo.codes` zone, taken over as it was added by hand. |
| `site.tf` | The redirect of `koyo.codes` and `www.koyo.codes` to the GitHub repository, and the two records it needs. There is no site yet. |
| `releases.tf` | The R2 bucket `koyo-releases`, and its public host name `releases.koyo.codes`. |
| `ci.tf` | A token that reads and writes that bucket's objects and nothing else, stored as the release workflow's secrets, and the bucket's addresses, stored as its variables. |

`.github/workflows/distribution.yml` plans every pull request that touches this folder, and applies `main`. What a release publishes there is in [Updates](../docs/updates.md), section 2.

## Set it up, once

Terraform cannot make the bucket its own state is in, or the tokens it runs with. Make those by hand.

1. Point the domain at Cloudflare. At the registrar of `koyo.codes`, replace the name servers with the two that Cloudflare assigned to the zone. The zone is `pending` until the registrar does, and the releases host cannot be made before it is `active`. It has been `active` since 2026-11-26.
2. In the Cloudflare dashboard, under **DNS**, delete the records that Cloudflare copied from the old host for `koyo.codes` and `www`. Nothing is behind them, and Terraform makes its own for those two names.
3. In the Cloudflare dashboard, under **R2**, create a bucket named `koyo-terraform-state`. Keep it private.
4. Under **R2**, **Manage API tokens**, create a token with **Object Read & Write** on `koyo-terraform-state` only. Store its key:

	```sh
	gh secret set DISTRIBUTION_STATE_ACCESS_KEY_ID -R repost-sh/koyo
	gh secret set DISTRIBUTION_STATE_SECRET_ACCESS_KEY -R repost-sh/koyo
	```

5. Under **Manage account**, **Account API tokens**, create a custom token with these permissions, and store it:

	| Scope | Permission |
	|---|---|
	| Account | Workers R2 Storage: Edit |
	| Account | Account API Tokens: Edit |
	| Zone, `koyo.codes` | Zone: Edit |
	| Zone, `koyo.codes` | DNS: Edit |
	| Zone, `koyo.codes` | Single Redirect: Edit |

	```sh
	gh secret set DISTRIBUTION_CLOUDFLARE_API_TOKEN -R repost-sh/koyo
	```

6. On GitHub, create a fine-grained personal access token for the repository `repost-sh/koyo` with **Secrets: Read and write** and **Variables: Read and write**. Store it:

	```sh
	gh secret set DISTRIBUTION_GITHUB_TOKEN -R repost-sh/koyo
	```

7. Merge this folder into `main`. The workflow applies it.

After the first apply, the release workflow has `KOYO_RELEASE_BASE`, `KOYO_BUCKET_URL`, `KOYO_BUCKET_ACCESS_KEY_ID`, and `KOYO_BUCKET_SECRET_ACCESS_KEY`. Do not set those by hand.

## Change it

Edit the `.tf` files and open a pull request. Read the plan in the **Plan** job's log, then merge.

The domain's own names lead to the repository with a temporary redirect (302), so that a site can take them later without browsers remembering the old answer. To give Koyo a site, replace `site.tf`.

To plan on your own Mac, export the same four values and run Terraform in this folder:

```sh
export CLOUDFLARE_API_TOKEN=… GITHUB_TOKEN=… AWS_ACCESS_KEY_ID=… AWS_SECRET_ACCESS_KEY=…
terraform init
terraform plan
```

Apply only through the workflow, so that `main` and what is provisioned stay the same.

## What cannot change

- **`releases.koyo.codes`.** Every released Koyo carries this host name in its bundle. A Koyo that is out reads its updates from there and from nowhere else.
- **The bucket and the zone.** Both have `prevent_destroy`. Terraform refuses a plan that would remove either.
