# Koyo's domain. It was added to Cloudflare by hand, so Terraform takes it over as it is and
# never removes it: removing a zone takes every record of the domain with it.
import {
  to = cloudflare_zone.koyo
  id = "db41e7b73bd7f7a31435e5b5e82f6ff8"
}

resource "cloudflare_zone" "koyo" {
  account = {
    id = var.account_id
  }
  name = var.zone
  type = "full"

  lifecycle {
    prevent_destroy = true
  }
}
