class WeztermK8sHelper < Formula
  desc "Helper CLI for wezterm-k8s-power to manage kubeconfigs per tab"
  homepage "https://github.com/a1ex-var1amov/wez-k8s-helper"
  url "https://github.com/a1ex-var1amov/wez-k8s-helper.git", :using => :git, :branch => "main"
  version "HEAD"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--path", "rust/wezterm-k8s-helper", "--root", prefix
    # cargo installs into prefix/bin automatically
  end

  test do
    system "#{bin}/wezterm-k8s-helper", "--help"
  end
end


