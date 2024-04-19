# Publisher

This documentation provides a quick guide to publish verification files for indexed data files on IPFS. 

## To publish a Bundle

To start, you would need to provide several configurations

### Requirements

Publisher must have read access to all files contained in the Bundle. The publisher publish 1 Bundle at a time and is not responsible for hosting the file after publishing. 

**Expectations**
1. For each file in the bundle, the publisher chunk the files into specified sizes and generate a hash for all the chunks. 
2. The publisher creates a file manifest containing information on the total number of bytes, chunk sizes, and an ordered list of chunk hashes. 
3. The publisher publishs individual file manifests, 
4. The publisher creates a bundle manifest containing information on the file names, file manfiest addresses, file types, and other meta descriptions.


### CLI usage

The publisher must provide a name for the bundle, filenames, file type, version, and path for read-access of the files.

Publishing separate files stored in the local file system
```
$ file-exchange publisher \
  --filenames example0017686312.dbin,example-create-17686085.dbin \
  local-files --main-dir ./example-file/
```

Publishing files/objects stored in a remote s3 bucket into a bundle, provide s3 and bundle configurations.
```
$ file-exchange publisher \
  --file-names example0017686312.dbin,example-create-17686085.dbin \
  --bundle-name "blah" \
  --file-type flatfiles \
  --file-version 0.0.0 \
  --description "random flatfiles" \
  object-storage --region ams3 \
   --bucket "contain-texture-dragon" \
   --access-key-id "DO0000000000000000" \
   --secret-key "secretttttttttt" \
   --endpoint "https://ams3.digitaloceanspaces.com" 
```

For more information 
```
$ file-exchange --help

Publisher takes the files, generate bundle manifest,
and publish to IPFS

Usage: file-exchange publisher [OPTIONS] <COMMAND>

Commands:
  local-files     
  object-storage  
  help            Print this message or the help
                      of the given subcommand(s)

Options:
      --yaml-store <YAML_STORE_DIR>
          Path to the directory to store the generated
          yaml file for bundle [env: YAML_STORE_DIR=]
          [default: ./example-file/bundle.yaml]
      --chunk-size <CHUNK_SIZE>
          Chunk size in bytes to split files (Default:
          1048576 bytes = 1MiB) [env: CHUNK_SIZE=]
          [default: 1048576]
      --filenames <FILE_NAMES>
          Name for the files to publish [env:
          FILE_NAMES=]
      --bundle-name <BUNDLE_NAME>
          Name for the bundle (later this can be
          interactive) [env: BUNDLE_NAME=]
      --file-type <FILE_TYPE>
          Type of the file (e.g., sql_snapshot,
          flatfiles) [env: FILE_TYPE=]
      --bundle-version <FILE_VERSION>
          Bundle versioning [env: FILE_VERSION=]
      --identifier <IDENTIFIER>
          Identifier of the file given its type
          (chain-id for firehose flatfiles, subgraph
          deployment hash for subgraph snapshots)
          [env: IDENTIFIER=]
      --start-block <START_BLOCK>
          Start block for flatfiles [env:
          START_BLOCK=]
      --end-block <END_BLOCK>
          End block for sql snapshot or flatfiles
          [env: END_BLOCK=]
      --description <DESCRIPTION>
          Describe bundle content [env: DESCRIPTION=]
          [default: ]
      --chain-id <NETWORK>
          Network represented in CCIP ID (Ethereum
          mainnet: 1, goerli: 5, arbitrum-one: 42161,
          sepolia: 58008 [env: NETWORK=] [default: 1]
  -h, --help
          Print help
```
