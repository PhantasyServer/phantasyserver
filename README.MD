# PhantasyServer

An attempt at creating a custom PSO2 server.

## Targeted versions

 * Global: 6.1253.3 (Steam Manifest ID: 4887283973834886137)
 * Vita: 6.1001.2

## Running
### Compiling the server

 1) Get the binaries:
   - Option 1: complied master branch
     1) Go to [GitHub actions](https://github.com/PhantasyServer/phantasyserver/actions/workflows/build.yml?query=branch%3Amaster)
     2) Select the latest workflow
     3) Download the `data` and the `pso2server` for your architecture
   - Option 2: complie yourself
     1) [Install](https://www.rust-lang.org/learn/get-started) rustc (at least 1.75) if not installed
     2) Clone the repository (preferably `develop` branch): `git clone --branch=develop https://github.com/PhantasyServer/phantasyserver.git`
     3) Compile the project: `cargo build --release`
     4) Compile server data: `cargo run --bin=data_compiler --release data`
     5) You'll find your binaries under `target/release`
 2) Copy the `master_ship.toml` and `ship.toml` config files to the directory with the binaries
 3) Enable auto ship registration by setting `registration_enabled = true` in the `master_ship.toml`
 4) Set the `master_ship` key in the `ship.toml` config to the IP address of the master ship (can be `127.0.0.1`, but not `localhost` due to IPv6)
 5) Start the `master_ship` then `pso2ship_server`

### Patching the PC version

 1) Rename or remove `steam_api64.dll` from the game folder
 2) Get the [injector](https://github.com/PhantasyServer/pso2-rsa-injector-classic) (download latest release and follow steps 3-5 on the readme)
 3) Modify the injector config to point to your master ship
```
replace_address = true
auto_key_fetch = true

[[addresses]]
old = "gs0100.westus2.cloudapp.azure.com"
new = "{master ship ip}"

[[addresses]]
old = "gs0200.westus2.cloudapp.azure.com"
new = "{master ship ip}"

[[addresses]]
old = "gs0300.westus2.cloudapp.azure.com"
new = "{master ship ip}"

[[addresses]]
old = "gs0400.westus2.cloudapp.azure.com"
new = "{master ship ip}"

[[addresses]]
old = "gs0500.westus2.cloudapp.azure.com"
new = "{master ship ip}"

[[addresses]]
old = "gs0600.westus2.cloudapp.azure.com"
new = "{master ship ip}"
```

### Patching the Vita version

 1) Install [FAGDec](https://github.com/TeamFAPS/PSVita-RE-tools) and [RePatch](https://github.com/dots-tb/rePatch-reDux0) on Vita
 2) Download both 6.30 and 6.31 patches (NoPayStation will help you)

Now you have two options:
 - Option a: Using the injector
  3) Copy the 6.30 patch to any location on Vita
  4) Using VitaShell and `Open decrypted` option copy the `eboot.bin` to the `rePatch/PCSG00141/` folder
  5) Install the [injector](https://github.com/PhantasyServer/pso2-rsa-injector-vita)
 - Option b: Manual way
  3) Copy the 6.30 patch to the `patch` folder on Vita
  4) Using FAGDec decrypt the `eboot.bin` to the ELF file
  5) Using VitaShell decrypt the `eboot.bin`
  6) Transform the server's public key to the compatible format: `openssl rsa -in keypair.pem -outform MS\ PUBLICKEYBLOB -pubout -out publickey.blob`
  7) Find and replace the RSA key in the `eboot.bin.elf` using any HEX editor (search for `0x06, 0x02, 0x00, 0x00, 0x00, 0xA4, 0x00, 0x00, 0x52, 0x53, 0x41, 0x31`)
  8) Using `vita-elf-inject` recreate the bin file: `vita-elf-inject eboot.bin.elf eboot.bin`
  9) Replace the 6.30 patch with 6.31 patch on the Vita
  10) Place the new `eboot.bin` in the `rePatch/PCSG00141/` folder
