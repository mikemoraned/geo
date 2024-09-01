<script>
	import { onMount } from 'svelte';

	onMount(async () => {
		const geo = await fetch('https://geo-api.houseofmoran.io/');
		console.log('geo', geo);
		const keks = await fetch('https://keks-api.houseofmoran.io/');
		console.log('keks', keks);
	});
</script>

<h1>Welcome to SvelteKit</h1>
<p>Visit <a href="https://kit.svelte.dev">kit.svelte.dev</a> to read the documentation</p>
