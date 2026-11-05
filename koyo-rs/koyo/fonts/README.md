# Bundled fonts

JetBrains Mono Nerd Font, which `src/theme/fonts.rs` embeds in the binary. The four faces are
unchanged from `JetBrainsMono.tar.xz` in
[Nerd Fonts v3.5.1](https://github.com/ryanoasis/nerd-fonts/releases/tag/v3.5.1), SHA-256
`04d5e8f903693f9dd13e16f867e994834e681eb3c72c0d337a770dcda09010cf`.

They're under the SIL Open Font License 1.1 (`OFL.txt`), which `script/bundle` copies into the
app. To update them, replace the faces with the same names from a newer release and update the
version and hash here.
