import { mount } from "svelte";
import Fixture from "./Fixture.svelte";
import Plugins from "./Plugins.svelte";
import GroupHistory from "./GroupHistory.svelte";
import GroupOverlays from "./GroupOverlays.svelte";
import DateJump from "./DateJump.svelte";

mount(location.search === "?group-history" ? GroupHistory : location.search === "?group-overlays" ? GroupOverlays : location.search === "?date-jump" ? DateJump : location.search === "?plugins" ? Plugins : Fixture, { target: document.getElementById("app")! });
