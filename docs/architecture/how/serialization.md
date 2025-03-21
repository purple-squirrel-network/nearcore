# Serialization: Borsh, Json, ProtoBuf

If you spent some time looking at NEAR code, you’ll notice that we have
different methods of serializing structures into strings. So in this article,
we’ll compare these different approaches, and explain how and where we’re using
them.

## JSON

JSON doesn’t need much introduction. We’re using it for external APIs (jsonrpc)
and configuration. It is a very popular, flexible and human-readable format.

## Proto (Protocol Buffers)

We started using proto recently - and we plan to use it mostly for our network
communication. Protocol buffers are strongly typed - they require you to create
a .proto file, where you describe the contents of your message.

For example:

```proto
message HandshakeFailure {
  // Reason for rejecting the Handshake.
  Reason reason = 1;

  // Data about the peer.
  PeerInfo peer_info = 2;
  // GenesisId of the NEAR chain that the peer belongs to.
  GenesisId genesis_id = 3;
}
```

Afterwards, such proto file is fed to protoc ‘compiler’ that returns an
auto-generated code (in our case rust code) - that can be directly imported into
your library.

The main benefit of protocol buffers is their backwards compatibility (as long
as you adhere to the rules and don’t reuse the same field ids).

## Borsh

Borsh is our custom serializer (<https://github.com/near/borsh>), that we use
mostly for things that have to be hashed.

The main feature of Borsh is that, there are no two binary representations that
deserialize into the same object.

You can read more on how Borsh serializes the data, by looking at Specification
tab on <https://borsh.io>

The biggest pitfall/risk of Borsh, is that any change to the structure, might
cause previous data to be no longer parseable.

For example, inserting a new enum ‘in the middle’ :

```rust
pub enum MyCar {
  Bmw,
  Ford,
}

If we change our enum to this:

pub enum MyCar {
  Bmw,
  Citroen,
  Ford, // !! WRONG - Ford objects cannot be deserialized anymore
}
```


This is especially tricky if we have conditional compilation:

```rust
pub enum MyCar {
  Bmw,
  #[cfg(feature = "french_cars")]
  Citroen,
  Ford,
}
```

Is such scenario - some of the objects created by binaries with this feature
enabled, will not be parseable by binaries without this feature.

Also removing and adding fields to structures is also dangerous.

Basically - the only ‘safe’ thing that you can do with Borsh - is add a new Enum
value at the end.

## Summary

So to recap what we’ve learned:

JSON - mostly used for external APIs - look for serde::Serialize/Deserialize

Proto - currently being developed to be used for network connections - objects
have to be specified in proto file.

Borsh - for things that we hash (and currently also for all the things that we
store on disk - but we might move to proto with this in the future). Look for
BorshSerialize/BorshDeserialize


## Questions

### Why don’t you use JSON for everything ?

While this is a tempting option, JSON has a few drawbacks:

* size (json is self-describing, so all the field names etc are included every time)
* non-canonical: JSON doesn’t specify strict ordering of the fields, so we’d
  have to do additional restrictions/rules on that - otherwise the same
  ‘conceptual’ message would end up with different hashes.

### Ok - so how about proto for everything?

There are couple risks related with using proto for things that have to be
hashed. A Serialized protocol buffer can contain additional data (for example
fields with tag ids that you’re not using) and still successfully parse (that’s
how it achieves backward compatibility).

For example, in this proto:

```proto
message First {
  string foo = 1;
  string bar = 2;
}
message Second {
  string foo = 1;
}
```

Every ‘First’ message will be successfully parsed as ‘Second’ message - which
could lead to some programmatic bugs.

## Advanced section - RawTrieNode

But there is one more place in the code, where we use a ‘custom’ encoding (very
similar to Borsh, but a little different): RawTrieNode

If you look into store/src/trie/mod.rs, you’ll be able to find a method called ‘encode_into’:

```rust
    fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), std::io::Error> {
        let mut cursor = Cursor::new(out);
        // size in state_parts = size + 8 for RawTrieNodeWithSize + 8 for borsh vector length
        match &self {
            // size <= 1 + 4 + 4 + 32 + key_length + value_length
            RawTrieNode::Leaf(key, value_length, value_hash) => {
                cursor.write_u8(LEAF_NODE)?;
                cursor.write_u32::<LittleEndian>(key.len() as u32)?;
                cursor.write_all(key)?;
                cursor.write_u32::<LittleEndian>(*value_length)?;
                cursor.write_all(value_hash.as_ref())?;
            }
         //... more code
```

which is responsible for generating this custom encoding.

If you analyse the code carefully - you’ll notice that this custom encoding
differs from Borsh in one place - in how it encodes ‘children’
(`[Option<CryptoHash>; 16]`) with a help of a bitmask, while Borsh would have
used a different layout.

Imagine a children vector like this:

```rust
[Some(0x11), None, Some(0x12), None, None, …]
```

Custom encoder:

```
// Number of children detetermined by the bitmask
[16 bits bitmask][32 bytes child][32 bytes child]
[3][0x11][0x12]
// Total size: 2 + 32 + 32 = 68 bytes
```

Borsh:

```
[8 bits - 0 or 1][32 bytes child][8 bits 0 or 1][8 bits ]
[1][0x11][0][1]][0x11][0][0]...
// Total size: 16 + 32 + 32 = 80 bytes
```
