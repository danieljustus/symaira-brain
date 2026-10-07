# Preserve the rejected second registry candidate

Original independent reviewer source:5ed6f1733f3bdaf86ef88eddb3616cece5431d8e.
The original review, receipt,120 raw CLI pairs (36 mismatches), six help pairs
(four mismatches), and original reproducer/log bytes are copied without edits.
Original executable retained at /tmp/symaira-registry-5ed-rust, SHA256
b513cf31821d77af6248f70f76d8fcbbb0b53219672cb7106093cb7c21c6ac2b.

The additional actual original5ed executable baseline runs the expanded probe
in private roots:360 invalid pairs,190 mismatches/170 matches; six supported-help
pairs,four mismatches/two matches. No daemon/profile/state files were created.
The probe hashes its own source, new test helper and both binaries, and labels
the author worktree dirty rather than pretending this is clean candidate proof.
Root-help comparison removes only the verified unsupported Go session-id row;
all other bytes are retained. The complete independent six-pair observation
also retains the unfiltered Go and native stdout bytes.

These files prove original failures. They do not approve the correction or waive
six native exact-head jobs, separate state-key work or final #772 acceptance.
