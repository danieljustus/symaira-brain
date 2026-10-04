# Resource decision: retain committed evidence, retire idle source checkouts

An isolated worktree is reconstructible from its retained ref and Git blobs. Retire only clean obsolete strict ancestors of published candidates after complete tracked-byte and native-mode validation, no cache/target/runtime ownership, and fresh visible consumer checks. Keep exact latest and active lanes. Stop at the agreed free-space budget instead of deleting the whole candidate list. Preserve original raw decisions, failures, compressed derivative bytes and restoration instructions. Privileged process visibility remains a separately recorded boundary.

The first ten retirements and this second three-tree run are separately journalled so concurrent cache cleanup is not counted twice.
