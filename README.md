# lexe-node
Enclave code including the managed LDK node + peripherals

## Installation
```
git clone https://github.com/MaxFangX/lexe-node
```

## Usage
```
cd lexe-node
cargo run <bitcoind-rpc-username>:<bitcoind-rpc-password>@<bitcoind-rpc-host>:<bitcoind-rpc-port> <ldk_storage_directory_path> [<ldk-peer-listening-port>] [bitcoin-network] [announced-listen-addr announced-node-name]
```
`bitcoin-network`: defaults to `testnet`. Options: `testnet`, `regtest`, and
`signet`.

`ldk-peer-listening-port`: defaults to 9735.

`announced-listen-addr` and `announced-node-name`: default to nothing, disabling
any public announcements of this node.
`announced-listen-addr` can be set to an IPv4 or IPv6 address to announce that
as a publicly-connectable address for this node.
`announced-node-name` can be any string up to 32 bytes in length, representing
this node's alias.

## License

MIT
