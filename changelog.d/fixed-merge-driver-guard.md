- **A test now blocks any merge driver returning to `.gitattributes`, in any
  directory.** Removing the last `merge=union` declaration stopped the phantom
  conflicts, but nothing stopped one being added back. The guard was widened
  from "the root file declares no union driver" to "no tracked `.gitattributes`
  declares any `merge=` driver": git honours a nested `.gitattributes` exactly
  as hard as the root one, and GitHub's merge machinery honours none of them, so
  `ours` and `theirs` diverge from the local result the same way union did. The
  failure names the offending file and line.
