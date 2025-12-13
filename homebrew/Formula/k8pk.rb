class K8pk < Formula
  desc "Kubernetes context picker - cross-terminal k8s context/namespace switcher"
  homepage "https://github.com/vee-sh/k8pk"
  url "https://github.com/vee-sh/k8pk.git", :using => :git, :branch => "main"
  version "HEAD"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--path", "rust/k8pk", "--root", prefix
    # cargo installs into prefix/bin automatically
    
    # Install shell completions
    generate_completions_from_executable(bin/"k8pk", "completions")
  end

  test do
    system "#{bin}/k8pk", "--help"
  end
end


