- **Publishing to a branch with auto-merge armed now cycles the promise instead
  of refusing the push.** Arming auto-merge is a promise to merge whatever is on
  the branch the moment checks go green, so pushing afterwards races it — PR #37
  stranded four commits that way, and PR #134 later stranded five more. Refusing
  the push held the invariant but made every follow-up commit a manual
  disarm/re-arm dance, and the escape it pushed people toward was arming late,
  which means somebody sitting and watching a pull request instead. The
  publisher now withdraws the promise, pushes, and re-makes it about the tip
  that was actually published: at no point is there an armed promise about a
  branch being written to, and nobody merges or disarms by hand. A push that
  fails still restores the promise, because a failed push leaves the branch
  exactly as the promise already described it; a re-arm that fails leaves the
  pull request disarmed and says so, which is the safe direction — work sits
  unmerged and visible rather than merging something nobody promised. The guard
  module also has tests now, having been written with the stated purpose of
  being testable without a network and then shipped with none.
