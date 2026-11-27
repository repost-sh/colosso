# What the release workflow needs to publish (docs/updates.md 5.5), made here and handed to
# the repository, so nobody copies a key by hand: a token that reads and writes the objects
# of the releases bucket and nothing else, and where that bucket is.

data "cloudflare_account_api_token_permission_groups_list" "bucket_item_write" {
  account_id = var.account_id
  name       = urlencode("Workers R2 Storage Bucket Item Write")
}

locals {
  bucket_item_write = one(data.cloudflare_account_api_token_permission_groups_list.bucket_item_write.result)

  # How Cloudflare names one bucket as a token's resource.
  releases_bucket_resource = "com.cloudflare.edge.r2.bucket.${var.account_id}_default_${cloudflare_r2_bucket.releases.name}"

  # The bucket as its S3 API takes writes (script/publish).
  releases_bucket_url = "https://${var.account_id}.r2.cloudflarestorage.com/${cloudflare_r2_bucket.releases.name}"
}

resource "cloudflare_account_token" "publish" {
  account_id = var.account_id
  name       = "koyo: publish releases"

  policies = [{
    effect = "allow"
    permission_groups = [{
      id = local.bucket_item_write.id
    }]
    resources = jsonencode({
      (local.releases_bucket_resource) = "*"
    })
  }]
}

# R2's S3 API signs with the token's id as the key's id, and the SHA-256 of the token's value
# as its secret.
resource "github_actions_secret" "bucket_access_key_id" {
  repository      = var.github_repository
  secret_name     = "KOYO_BUCKET_ACCESS_KEY_ID"
  plaintext_value = cloudflare_account_token.publish.id
}

resource "github_actions_secret" "bucket_secret_access_key" {
  repository      = var.github_repository
  secret_name     = "KOYO_BUCKET_SECRET_ACCESS_KEY"
  plaintext_value = sha256(cloudflare_account_token.publish.value)
}

resource "github_actions_variable" "release_base" {
  repository    = var.github_repository
  variable_name = "KOYO_RELEASE_BASE"
  value         = "https://${cloudflare_r2_custom_domain.releases.domain}"
}

resource "github_actions_variable" "bucket_url" {
  repository    = var.github_repository
  variable_name = "KOYO_BUCKET_URL"
  value         = local.releases_bucket_url
}
