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
        # TODO: Use another env var
        shellHook = ''
          export CLAUDE_CONFIG_DIR="$HOME/.claude-personal"
        '';

        buildInputs = [
          pkgs.postgresql
          pkgs.mysql84
        ];
      };
    };
}
