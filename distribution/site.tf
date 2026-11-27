# The domain has no site of its own: koyo.codes and www.koyo.codes lead to the repository,
# until there is one. Cloudflare answers the redirect at its edge, before any origin, so the
# two names need only pass through it: their records point at the address reserved for
# nothing.

locals {
  repository_url = "https://github.com/${var.github_owner}/${var.github_repository}"
  site_hosts     = [var.zone, "www.${var.zone}"]
}

resource "cloudflare_dns_record" "site" {
  for_each = toset(local.site_hosts)

  zone_id = cloudflare_zone.koyo.id
  name    = each.value
  type    = "AAAA"
  content = "100::"
  proxied = true
  ttl     = 1
  comment = "No origin: this name is redirected to the repository (distribution/site.tf)."
}

# Found, not moved for good: a site of Koyo's own may take the domain later, and browsers
# keep a permanent redirect.
resource "cloudflare_ruleset" "redirects" {
  zone_id     = cloudflare_zone.koyo.id
  name        = "Redirects"
  description = "Where the domain's own names lead."
  kind        = "zone"
  phase       = "http_request_dynamic_redirect"

  rules = [{
    ref         = "site_to_repository"
    description = "The domain leads to the repository."
    expression  = join(" or ", [for host in local.site_hosts : "(http.host eq \"${host}\")"])
    action      = "redirect"
    action_parameters = {
      from_value = {
        status_code           = 302
        preserve_query_string = false
        target_url = {
          value = local.repository_url
        }
      }
    }
  }]

  depends_on = [cloudflare_dns_record.site]
}
