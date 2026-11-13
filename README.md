# Chia testnet monitor

A live monitor for your farm on Chia's ephemeral PoS2 testnets. It counts down to the hard fork,
shows every block from genesis to the v1 cut-off by plot version, and splits your plots,
proofs and rewards by v1 and v2. Wins get confetti.

![Dashboard](screenshot.png)

## Run it

You need the `hardfork-testnet` branch of chia-blockchain installed and a node running, as in the
testnet instructions. Clone this repo once:

```sh
git clone https://github.com/greimela/chia-testnet-monitor
```

Then, in the same terminal you use for `chia`:

```sh
cd chia-blockchain
. ./activate
export CHIA_ROOT=~/.chia/<network_name>
python /path/to/chia-testnet-monitor/server.py --open
```

That opens http://127.0.0.1:8899. Use `--port 9000` if 8899 is taken, and Ctrl-C to stop.

When you move to a new testnet, restart it with the new `CHIA_ROOT`.

## What it reads

- The full node, farmer, harvester and wallet RPCs, using the certificates in your `CHIA_ROOT`.
- `log/debug.log`, for signage points, proofs, warnings and errors. Set the log level to INFO
  (`chia configure --set-log-level INFO`) or the signage-point chart stays empty.

Only the full node and farmer have to be running; the wallet, harvester and solver show up when they are.

## Good to know

- It only reads. It never sends a transaction or changes your config.
- The page is served on 127.0.0.1 only, so other machines on your network can't open it.
- v2 plots don't pass the filter until the hard fork height, so "v2 qualities" stays at 0 until then.
- v2 plots need the solver: run `chia_solver`, then
  `chia rpc farmer connect_to_solver '{"host": "localhost", "port": 8666}'`.
  The dashboard warns you if you have v2 plots and the solver isn't connected.
- No dependencies beyond what chia-blockchain already installs. The page loads the Archivo font from
  Google Fonts and canvas-confetti from jsDelivr.

## License

Apache 2.0, like chia-blockchain. See [LICENSE](LICENSE).
