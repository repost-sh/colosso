terraform {
  required_version = ">= 1.11"

  required_providers {
    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "~> 5.29"
    }
    github = {
      source  = "integrations/github"
      version = "~> 6.13"
    }
  }

  # The state is kept in an R2 bucket of its own, which Terraform can't make for itself: the
  # README says how it's made, once. R2 answers the S3 API, and asks for none of the checks
  # that are Amazon's alone.
  backend "s3" {
    bucket = "koyo-terraform-state"
    key    = "distribution/terraform.tfstate"
    region = "auto"

    endpoints = {
      s3 = "https://52faf88a8217bdef95fefa1b2c920f72.r2.cloudflarestorage.com"
    }

    skip_credentials_validation = true
    skip_metadata_api_check     = true
    skip_region_validation      = true
    skip_requesting_account_id  = true
    skip_s3_checksum            = true
    use_path_style              = true
  }
}

# CLOUDFLARE_API_TOKEN in the environment.
provider "cloudflare" {}

# GITHUB_TOKEN in the environment.
provider "github" {
  owner = var.github_owner
}
