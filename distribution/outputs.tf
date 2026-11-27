output "name_servers" {
  description = "The name servers the domain's registrar must point at for the zone to become active."
  value       = cloudflare_zone.koyo.name_servers
}

output "zone_status" {
  description = "active once the registrar points at the name servers above; pending until then."
  value       = cloudflare_zone.koyo.status
}

output "release_base" {
  description = "Where released Koyos read their updates from."
  value       = github_actions_variable.release_base.value
}

output "releases_host_status" {
  description = "Whether Cloudflare has taken the releases host: its ownership and its certificate."
  value       = cloudflare_r2_custom_domain.releases.status
}

output "site" {
  description = "Where koyo.codes and www.koyo.codes lead."
  value       = local.repository_url
}
