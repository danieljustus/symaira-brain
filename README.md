# MacBook handoff proof archive (not a release)

These ordered parts contain a curated original-proof archive. Read docs/handoff/macbook-20261004.md on handoff/20261004/coordination before using it. No production implementation or workflow is included on this export branch.

Inspect parts-manifest.json and verify each part with shasum -a 256. Concatenate with:

```bash
cat symaira-macbook-proofs-20261004.tar.gz.part* > symaira-macbook-proofs-20261004.tar.gz
shasum -a 256 symaira-macbook-proofs-20261004.tar.gz
tar -tzf symaira-macbook-proofs-20261004.tar.gz
mkdir extracted
tar -xzf symaira-macbook-proofs-20261004.tar.gz -C extracted
```

The archive contains docs/handoff/proof-export-manifest.json with every included proof SHA256 and explicit exclusions. It is a partial export, not a full SDK/Target/binary archive. Scripts must not be executed without review and fresh host/source binding.
