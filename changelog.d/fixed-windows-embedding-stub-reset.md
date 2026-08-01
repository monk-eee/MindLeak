- **Windows Rust CI no longer resets the embedding fixture mid-request.** The
  Lodestar embedding tests now use the shared model endpoint, which consumes
  complete HTTP headers and the declared body before replying and closing.
