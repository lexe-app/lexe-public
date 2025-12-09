# copy the fake SGX libc shim into the nix store.
# placing the `sgx-libc-shim` in its own derivation seems to stop needless
# rebuilds.
{ stdenvNoCC }:
stdenvNoCC.mkDerivation {
  name = "sgx-libc-shim";
  src = ../../sgx-libc-shim;
  dontUnpack = true;
  installPhase = ''
    mkdir -p $out
    cp -r $src/include $out/
  '';
}
