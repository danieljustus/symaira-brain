# Preserve runtime evidence while managing compiler storage

Runtime work must measure each output/cache filesystem, not only its target.
The dedicated `/tmp` mount became full while the workspace still had space.
This caused actual Doctor/Memory oracle SDK builds to fail before comparison;
those failures remain retained and are not classified as product parity.
Use owned workspace output/TMPDIR paths and check actual headroom before stages.

The original workspace target's inactive incremental cache was audited before
retirement. Every ELF was a relocatable compiler object (ET_REL), with no
executable/shared object and zero process executable/cwd/FD users. Complete
path/size/hash metadata was preserved. Source, runnable/build/dependency files
and all other targets/evidence remained untouched. Removing2,243,275,811 bytes
of visible derivative objects did not reclaim overlay disk space; measured
available bytes fell slightly from910,295,040 to905,760,768 due to the manifest.
Treat that as no headroom gained, rather than assuming directory sizes predict
physical storage reclamation.

The Discovery owner separately verified all37 current Linux role/archive hashes
and retired only its completed Windows/Darwin static-check caches. Those167MB
of source-check derivatives contained no direct executable ELF/PE files and
actually reclaimed173,146,112 filesystem bytes. Linux executables and the
exclusive runtime target remain available. Neither action establishes2GiB
headroom for a new heavy compiler stage. Keep the700MiB runtime floor and
reserve sufficient additional build/output capacity before restarting.

Decisions favor reproducible cache retirement over losing original failures or
runtime proof. Obsolete clean worktrees may be retired only after checking
inactive ownership and complete committed source/evidence ancestry; active
targets and uncommitted work are preserved. Every claimed storage improvement
must use the measured free-byte result. Full Root resource audit is retained
with the current Memory checker/independent evidence.
