{
  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
    in
    {
      formatter.x86_64-linux = (
        pkgs.nixpkgs-fmt
      );

      devShells.x86_64-linux.default = pkgs.mkShell {
        packages = [
          pkgs.cargo-audit
          pkgs.postgresql
          pkgs.mysql84
        ];
      };
    };
}
