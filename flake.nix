{
  description = "k8pk — Kubernetes context picker";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f system);
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          manifest = (builtins.fromTOML (builtins.readFile ./rust/k8pk/Cargo.toml)).package;
        in
        {
          default = self.packages.${system}.k8pk;
          k8pk = pkgs.rustPlatform.buildRustPackage {
            pname = "k8pk";
            version = manifest.version;
            src = ./rust/k8pk;
            cargoLock = {
              lockFile = ./rust/k8pk/Cargo.lock;
            };
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = [ ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
            meta = with pkgs.lib; {
              description = manifest.description;
              homepage = manifest.repository;
              license = licenses.mit;
              mainProgram = "k8pk";
            };
          };
        });
    };
}
