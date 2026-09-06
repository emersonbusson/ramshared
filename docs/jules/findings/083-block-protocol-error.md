# FINDING_ONLY: ProtocolError already provides semantic types

The file crates/ramshared-block/src/protocol.rs already perfectly implements precise semantic error returns within the existing ProtocolError enum. The parsing logic inside parse_request already validates inputs and returns these specific domain-typed variants. Altering this functioning code to duplicate these types or forcefully return raw EINVAL numbers when a rich domain enum is already used would violate the existing correct application of the Specific & Semantic Errors principle.
