# Token is the product name. scripts/bump-homebrew-cask.sh rewrites the
# version, sha256, url, app, quit, zap, caveat, and livecheck stanzas from
# one Release disk image; livecheck follows GitHub's latest Release.
cask "fidget" do
  version "0.8.0"
  sha256 "6e98abd6bfd9d8937100ac448f3b5f2072152c65f6bc8e80377c0a3427f64c73"

  url "https://github.com/omesser/fidget/releases/download/v#{version}/Fidget_#{version}_aarch64.dmg"
  name "Fidget"
  desc "Desktop companion that lives on your screen"
  homepage "https://github.com/omesser/fidget"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on arch: :arm64
  depends_on :macos

  app "Fidget.app"

  uninstall quit: "dev.omesser.fidget"

  zap trash: [
    "~/Library/Application Support/fidget",
  ]

  caveats <<~EOS
    This build installs Fidget. It is not signed. Dismiss the Gatekeeper
    dialog, then System Settings → Privacy & Security → Open Anyway.
  EOS
end
