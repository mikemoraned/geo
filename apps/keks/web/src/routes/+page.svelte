<script>
	import { onMount } from 'svelte';

	onMount(async () => {
		const geo = await fetch('https://geo-api.houseofmoran.io/');
		console.log('geo', geo);
	});
</script>

<h1>Welcome to SvelteKit</h1>
<p>Visit <a href="https://kit.svelte.dev">kit.svelte.dev</a> to read the documentation</p>
