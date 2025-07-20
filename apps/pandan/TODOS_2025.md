# July/August

* [ ] get to equivalent-level functionality as https://spectrum.houseofmoran.io but using newer stuff
    * [ ] define greenery
        * [ ] find thing in OvertureMaps that corresponds to [GreenTags](https://github.com/mikemoraned/spectrum/blob/main/app/service/builder/src/filter.rs#L7)
        * ...
    * [ ] lookup route between points in a way which is biased towards walking
        * ...
    * [ ] do intersection of route with greenery area, client-side in browser, and display
        * ...

* [ ] refactor for sharing:
    * [x] extract out shared config and overturemaps crates
    * [x] get pandan using them in a minimal way
    * [ ] change linzer to use those instead of it's own
    * [ ] get rid of crufty linzer crates that are unused / incomplete