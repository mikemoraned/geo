// Fills in the build's git hash wherever a page has somewhere to put it, so what is running
// can be matched to the source it came from. Every page carries it, so every page loads this.
const shown = document.querySelector("#version");

if (shown) {
  fetch("/version")
    .then((response) => response.text())
    .then((hash) => {
      shown.textContent = hash;
    })
    .catch(() => {
      shown.textContent = "unknown";
    });
}
