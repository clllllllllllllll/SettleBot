// Test-only RPC bridge. The normal Worker build excludes this Rust export.
import RustWorker from "../build/d1-test/index.js";

export default class PersistenceTests extends RustWorker {
  command(input) {
    return super.persistence_test_command(input, this.env);
  }
}
