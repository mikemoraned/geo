<script>

	import Map from '../Map.svelte';
</script>

<head>
	<title>geoarrow.info</title>
</head>

<div id="drop_zone">Drag and drop files here</div>
<Map />

<style>
    #drop_zone {
        width: 300px;
        height: 200px;
        border: 2px dashed #007BFF;
        border-radius: 5px;
        text-align: center;
        line-height: 200px;
        color: #007BFF;
        font-family: Arial, sans-serif;
    }
</style>