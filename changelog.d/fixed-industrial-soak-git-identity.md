- Endurance runs now check the requested source checkout even when the parent
  process supplies Git repository pointers. Foreign Git metadata cannot replace
  the recorded commit and tree or hide tracked and untracked source changes;
  failed Git reads still prevent a passing qualification.
