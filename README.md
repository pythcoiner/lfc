# Limited Flow Channel

![limited_flow_channel](./assets/limited_flow_channel.png)

## Architecture

For a simple example we will assume our rate limited spending condition is "signature from A"
and our non rate limited spending condition is "signature from B ".

### S spend policy

For our simple example `S` will be defined as `pk(A)`

### V covenant policy

For our simple example `V` will be defined as `pk(B)`

## Usage

contract configuration must be stored in a file, an example can be
found at [conf_template](./contrib/lfc.conf)

Simply run this command to create a contract (regtest):

```
lfc <config_path>
```

you should get something like:

```
Configuration: 
 {
  "cov_mnemonic": "dice ghost nuclear autumn mixed come bubble sign fold short theme betray",
  "spend_mnemonic": "rigid engine whale execute panic fossil puppy clay syrup sausage broom shell",
  "amount": 10000000,
  "delay": 100,
  "index": 0
}
cov_descriptor: 
 
wpkh([cb9debb0/84'/1'/0']tpubDCpNQyLf8GK7BYTM6mLSWPjicEmSFGbLCRmzHBk6mT8vuTqCwZXPDjGoeJ5N7BcLfW4UiNHnQpThyRbZdf441JuvoXQN6d2ZqBSE1F7KQgs/0/*)#e79fpkcm 
 

spend_descriptor: 
 
wpkh([da09ded1/84'/1'/0']tpubDCpYMctBMP7P8M4AC1wg9eDVZHyFf8Z9XbB8t7a5enjJsTHYVNmD8AbryuyEoRMUy11mLMQAsZ79T8dxN9bhEgkZwXBG1E9TkCC8a8hMxio/0/*)#0qzrs6cz 
 

Address to fund the contract: bcrt1qvfrj0t5sjp55w3etl8arkpclfmq7cadqxukug4
```

 - import both descriptors & mnemonic in Sparrow in order to sign transactions

 - send some sats to the contract at the given address (`bcrt1qvfrj0t5sjp55w3etl8arkpclfmq7cadqxukug4` here)
 from another waller & broadcast the transaction (the transaction MUST have a single output to this address).

 - generate some blocks

 - copy the raw transaction in the terminal :

```
02000000000101a00f4a444b47605752200698dcaec9f08273e666dbace2713af92693b47f972c0000000000fdffffff0162c40d0300000000160014624727ae90906947472bf9fa3b071f4ec1ec75a002473044022076b13ee593a41205552d239ec61c56f72cf309f71ef8f2a8133cfa3914c5e5eb02200c2baf3986b2107610dd23c979a9083f3500e767be3c49bd2e66e69e31a5e2aa012103fbf952f603363cece17e06be0439638de28d2076216673a928353935da645e0fe0130000
```

 - transaction to presign will be generated:

```

Amount to split: 51233890
craft_tx(index: 1, spend: 10000000, relock: 41233890)
craft_tx(index: 2, spend: 10000000, relock: 31233890)
craft_tx(index: 3, spend: 10000000, relock: 21233890)
craft_tx(index: 4, spend: 10000000, relock: 11233890)
craft_tx(index: 5, spend: 10000000, relock: 1233890)
craft_tx(index: 6, spend: 1233290, relock: 0)
6 unsigned psbts: 

psbt 1: 
 cHNidP8BAHECAAAAARNLfHXAIJIGFF0Ovhn+kIgI7bnQ+ofhTFEJvybqKynaAAAAAAAAAAAAAuItdQIAAAAAFgAUHjPvOuGsvYzYQXvPNqZ4nFfKOpiAlpgAAAAAABYAFGWqqr6GVybDmR2eX7/TvOwV1Ug0AAAAAAABAR9ixA0DAAAAABYAFGJHJ66QkGlHRyv5+jsHH07B7HWgIgYDOtmPeM00Uqdrc09ADaBVXek2NC09sdU5Gszkrqfi1mkYy53rsFQAAIABAACAAAAAgAAAAAAAAAAAACICAsQVm3C5sEOgknID1fozOx4uE+8a3cRMKI21LF/5k8T5GMud67BUAACAAQAAgAAAAIAAAAAAAQAAAAAA 

psbt 2: 
 cHNidP8BAHECAAAAAc+yme77f3sWC2XjnBtQ4/aCnxJUn/netZtK2fY90oe6AAAAAABkAAAAAmKX3AEAAAAAFgAUsFnZu2spac0XFVeMSqWZZeaYRt6AlpgAAAAAABYAFED9ga53avzq5PzqQaJ4k8mQh01+AAAAAAABAR/iLXUCAAAAABYAFB4z7zrhrL2M2EF7zzameJxXyjqYIgYCxBWbcLmwQ6CScgPV+jM7Hi4T7xrdxEwojbUsX/mTxPkYy53rsFQAAIABAACAAAAAgAAAAAABAAAAACICA+lRCWS72mG61AEgCaooFh4WAdJYvjaqRQVT4eMy9gbsGMud67BUAACAAQAAgAAAAIAAAAAAAgAAAAAA 

psbt 3: 
 cHNidP8BAHECAAAAAUYI3D0Rz2YAGL9yMhX6GzuSjLMnZcYWp9vDitlPV2EZAAAAAABkAAAAAuIARAEAAAAAFgAUk18V2E7c84mElpM320deG4zbkbOAlpgAAAAAABYAFHFDx1Di+8Z3Q5RUDLLQjDOMDkEmAAAAAAABAR9il9wBAAAAABYAFLBZ2btrKWnNFxVXjEqlmWXmmEbeIgYD6VEJZLvaYbrUASAJqigWHhYB0li+NqpFBVPh4zL2BuwYy53rsFQAAIABAACAAAAAgAAAAAACAAAAACICA7U7pER8ByrhkXH7JsznfHhd1MrEy2/qARn45vYJlE48GMud67BUAACAAQAAgAAAAIAAAAAAAwAAAAAA 

psbt 4: 
 cHNidP8BAHECAAAAAR1eixuaBgQak4gM3FOrmwCESWCfbMv6TlQIJ00GgswwAAAAAABkAAAAAmJqqwAAAAAAFgAUw/dYlbRr0EMTA067fQTwiiaHTNKAlpgAAAAAABYAFN7krNQS9EPgLi4z5Dhcie+xuNLZAAAAAAABAR/iAEQBAAAAABYAFJNfFdhO3POJhJaTN9tHXhuM25GzIgYDtTukRHwHKuGRcfsmzOd8eF3UysTLb+oBGfjm9gmUTjwYy53rsFQAAIABAACAAAAAgAAAAAADAAAAACICAsNCxl2/WTLH/MmLRW8STO/jPNiL35oNlAJZUhW6ONGeGMud67BUAACAAQAAgAAAAIAAAAAABAAAAAAA 

psbt 5: 
 cHNidP8BAHECAAAAAaFjbq16RQN+NjkHohIkN/m9CW5C5DOLJqVRGP9CRs9fAAAAAABkAAAAAuLTEgAAAAAAFgAUvqWxmgfqAGAThEK9nBnSfqrwapKAlpgAAAAAABYAFMXrvgsCd+0Wx50Co3Hm6TOJf+MdAAAAAAABAR9iaqsAAAAAABYAFMP3WJW0a9BDEwNOu30E8Iomh0zSIgYCw0LGXb9ZMsf8yYtFbxJM7+M82Ivfmg2UAllSFbo40Z4Yy53rsFQAAIABAACAAAAAgAAAAAAEAAAAACICAiC2lqtbuHboJRoqVpsOVbjPbXAOVhtzIIxpELJYgnglGMud67BUAACAAQAAgAAAAIAAAAAABQAAAAAA 

psbt 6: 
 cHNidP8BAFICAAAAAdWLEAplAZx9j4lWK7cQLFvo6jw5rx+0/cLDbVPmnWJAAAAAAABkAAAAAYrREgAAAAAAFgAUSigjwYmX7JQxmoSL1JXt2i0jc/UAAAAAAAEBH+LTEgAAAAAAFgAUvqWxmgfqAGAThEK9nBnSfqrwapIiBgIgtparW7h26CUaKlabDlW4z21wDlYbcyCMaRCyWIJ4JRjLneuwVAAAgAEAAIAAAACAAAAAAAUAAAAAAA== 
```

 - you can now presign all these transactions w/ the `cov_descriptor` wallet from sparrow
 & store a copy of the presigned transactions.

 - you can now broadcast the transaction from `psbt1` directly and spend the coin w/ the spending key
 from sparrow.

 - generate few (10) blocks

 - try to broadcast the transaction from `psbt2` from sparrow, you'll get an error.

 - try to broadcast from `bitcoin-cli`:

```

$ bitcoin-cli -regtest sendrawtransaction 02000000000101cfb299eefb7f7b160b65e39c1b50e3f6829f12549ff9deb59b4ad9f63dd287ba000000000064000000026297dc0100000000160014b059d9bb6b2969cd1715578c4aa59965e69846de809698000000000016001440fd81ae776afceae4fcea41a27893c990874d7e0247304402202faf047ea3ca7350ea6506b9b5c3e980ab084e72199c54771e800a71a40a7e7e02201def279e929682c73fdf2be0ef9cd2ed604729a16f8c46061137da3930cb10ef012102c4159b70b9b043a0927203d5fa333b1e2e13ef1addc44c288db52c5ff993c4f900000000
```
 - you'll get an error: nSequence of the transaction input is set to `100` so you'll have to wait until 
 100 blocks after the `psbt1` transaction have been confirmed in order to broadcast the transaction 
 from `psbt2` and so on..

```
error code: -26
error message:
non-BIP68-final
```
 - generate 90 more blocks and then you can broadcast transaction from `psbt2`

 etc...
