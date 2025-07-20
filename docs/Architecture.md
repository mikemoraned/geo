# Code

* Shared Rust crates are kept in top-level `shared` directory with one sub-directory for each separate crate
* Apps sit under `apps` and depend on these shared crates via a local `path` in the `dependencies` section
* Config that is useful across apps is in the top-level `config` dir
* Experimental code is under the `spikes` dir and is not expected to always build i.e. it is throwaway

