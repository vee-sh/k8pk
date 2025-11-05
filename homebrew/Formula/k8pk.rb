class K8pk < Formula
  desc "Kubernetes context picker - cross-terminal k8s context/namespace switcher"
  homepage "https://github.com/a1ex-var1amov/k8pk"
  url "https://github.com/a1ex-var1amov/k8pk.git", :using => :git, :branch => "main"
  version "HEAD"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--path", "rust/k8pk", "--root", prefix
    # cargo installs into prefix/bin automatically
  end

  test do
    system "#{bin}/k8pk", "--help"
  end
end


