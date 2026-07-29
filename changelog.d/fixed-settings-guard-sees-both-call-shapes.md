- **The settings guard now sees every way a setting is read.** A test already
  failed when the extension read a setting the manifest never declared — an
  undeclared setting silently returns its inline fallback, so it cannot be found
  in the settings UI and appears to do nothing when a user sets it in JSON. But
  the scrape matched only reads through a `config` variable, and the extension
  also reads settings inline off `getConfiguration("mindleak")`. Two of the
  eighteen — `captureCommits` and `snapshotLimit` — were therefore invisible to
  it, so for those the guard could not have caught the mistake it exists to
  catch. Both call shapes are now scanned. The "guards the guard" check was a
  floor of "more than five", which 16 of 18 satisfied while two went unchecked;
  it now names a read of each shape, so the blind spot cannot quietly reopen.
