variable "account_id" {
  description = "The Cloudflare account that owns the zone and the buckets."
  type        = string
  default     = "52faf88a8217bdef95fefa1b2c920f72"
}

variable "zone" {
  description = "Koyo's domain."
  type        = string
  default     = "koyo.codes"
}

variable "releases_host" {
  description = "The host name every released Koyo reads its updates from. It is written into each release's bundle, so it cannot change once a release is out."
  type        = string
  default     = "releases.koyo.codes"
}

variable "releases_bucket" {
  description = "The R2 bucket that holds the releases."
  type        = string
  default     = "koyo-releases"
}

variable "github_owner" {
  description = "The GitHub organization of the repository that releases Koyo."
  type        = string
  default     = "repost-sh"
}

variable "github_repository" {
  description = "The repository that releases Koyo, whose release workflow is given the bucket."
  type        = string
  default     = "koyo"
}
