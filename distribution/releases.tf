# Where releases are published (docs/updates.md 2): one R2 bucket, served under a host name of
# the domain. Each release is a disk image written once and a small feed replaced last.
resource "cloudflare_r2_bucket" "releases" {
  account_id = var.account_id
  name       = var.releases_bucket
  location   = "weur"

  # Every Koyo that is out reads this bucket. An image is never removed.
  lifecycle {
    prevent_destroy = true
  }
}

# The bucket's public address. Cloudflare makes the DNS record and the certificate itself.
resource "cloudflare_r2_custom_domain" "releases" {
  account_id  = var.account_id
  bucket_name = cloudflare_r2_bucket.releases.name
  zone_id     = cloudflare_zone.koyo.id
  domain      = var.releases_host
  enabled     = true
  min_tls     = "1.2"
}
