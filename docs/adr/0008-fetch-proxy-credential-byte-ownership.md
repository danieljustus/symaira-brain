# Fetch: own transport proxy credentials as bytes

Decision: retain original proxy userinfo separately from the parsed policy/dial URL, split at the first literal colon, percent-decode into bytes, and construct Basic authorization for each HTTP-target hop. Append after caller headers and derive anew after redirects. Empty userinfo retains its presence.

Reason: URL/string normalization can erase invalid UTF-8 credentials. Keeping authentication bytes under the transport owner preserves Go behavior without coupling proxy and origin authentication or exposing secrets in Debug output. The existing locked percent/base64 codecs suffice; no new package version is introduced.

Independent approval: source c7beb3a7a39dbc10f7e84769fccea5cf20e7f09e, evidence head5b94a66159db34d335bc32b98bbcd2cb1dfbe5d9. Full review reran251 original pairs,114 ordered auth pairs,8 enforcing peers,123 tests and140 new actual wire comparisons. All pass; original four origin-userinfo gaps remain explicitly recorded. Full report and44 fresh artifacts are retained under browse/port/evidence/fetch-control-773/independent-proxy-auth-c7.

The unchanged value comparator was independently recalculated over all240 retained samples; Fetch p95 improves55.17%. This does not erase raw-route pooling cost:30 native connections versus1 Go per30 requests. Debug process-visible intervals are not isolated transport latency. Preserve those observations and pursue reuse separately.

Before merge require protected checks and genuine native six-platform CI at the published head. Full issue773/default cutover, CONNECT/SOCKS/H2/trailer equivalence and macOS/Windows trusted-CA success remain open. Linux evidence does not substitute for another backend. No broader parity exception is accepted.

Receipt retention correction: the review generator initially hashed its own completion log while empty, then wrote208bytes of completion output. Preserve the original receipt and independent supplement; all43 other evidence files are unchanged. This changes no production source, review result or performance observation.
