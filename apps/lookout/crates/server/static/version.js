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
