# Current Slice: Show map context within App itself

### Target

* show the train lines in current region of display: this shows which trainlines are nearby and so which are we likely to be on
* show the moving window of last N samples seen as a "Snake" style: this provides context of recent movements

### Straw Man

Building on what we did in in "Slice: Spike on Maps using Kiosk display as an example", we'd add ability of the App to request map data for a region and then return that to the View.

The lookup would be done by something embedded in Web Shell which can query protomaps or similar (whatever we decided to use based on the Spike). 

This lookup is optional in that on the M5 it likely won't have access to map data live and so would have to be embedded alongside. This is of limited value right now as the M5 has a very limited display and so can't really show a Map. Later on we may use devices that have a better display but for now it adds nothing.

The display can be kept quite dumb and just given a list of lines to display, named by category. Coloring can then be decided by category. For example, the View can return a line with a category of "trace" which is the recent positions of the device from GPS, and separately a set of lines with a category of "trainline" which are the local train lines we received.

