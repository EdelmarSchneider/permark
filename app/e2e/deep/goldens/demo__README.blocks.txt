# demo/README.md
format: markdown
outline:   Welcome aboard (line 15)
outline:   The tour (line 24)
outline:   The crew (line 41)
outline:   Find your way (line 65)
--- p 1-1
<p dir="auto"><a id="top" name="top"></a></p>
--- p 1-1
<p align="center" dir="auto">
  <img alt="Tidewell emblem: an amber buoy with a teal light riding three waves inside a compass ring" src="pm-asset:demo/assets/tidewell-emblem.svg" width="168">
</p>
--- h1 1-1
<h1 align="center" dir="auto">Tidewell Field Notes</h1>
--- p 1-1
<p align="center" dir="auto"><em>Twelve buoys, one small lab, and every page a plain Markdown file.</em></p>
--- p 1-1
<p align="center" dir="auto">
  <a href="field-journal.md"><strong>Start reading</strong></a> · <a href="station-architecture.md">Diagrams</a> · <a href="ocean-chemistry.md">Math</a> · <a href="tidewell-api.md">API</a> · <a href="season-plan.md">Plan</a> · <a href="bulletin-agent.md">Prompts</a>
</p>
--- h2 15-15
<h2 dir="auto" id="welcome-aboard">Welcome aboard</h2>
--- p 17-17
<p dir="auto">Larkhaven Marine Lab runs Tidewell: twelve solar buoys on Larkhaven Sound that measure temperature, salinity, pH, dissolved oxygen and waves every ten minutes, and publish all of it as open data. This folder is the lab's notebook: the voyage that put Season 3 in the water, the science behind the numbers, the API, the plan and the people.</p>
--- p 19-19
<p dir="auto">It is also a sample project for PerMark. Every page here is a plain Markdown file you could open in any text editor. The diagrams, formulas, callouts and tables you're about to see are all written as text.</p>
--- div 21-22
<div class="markdown-alert markdown-alert-tip">
<p class="markdown-alert-title" dir="auto">This folder is yours</p>
<p dir="auto">Edit any page, break it, delete it. Nothing here is precious, and PerMark only ever saves the parts you change.</p>
</div>
--- h2 24-24
<h2 dir="auto" id="the-tour">The tour</h2>
--- p 26-26
<p dir="auto">Ten pages, each one a real document from the lab, and each showing off something different.</p>
--- table 28-39
<table>
<thead>
<tr>
<th align="center" dir="auto"></th>
<th dir="auto">Page</th>
<th dir="auto">What you'll see</th>
</tr>
</thead>
<tbody>
<tr>
<td align="center" dir="auto"><span class="emoji" title=":sailboat:">⛵</span></td>
<td dir="auto"><a href="field-journal.md">Nine Days on the Petrel</a></td>
<td dir="auto">A voyage journal with a map, footnotes, a sound clip and a letter in Portuguese</td>
</tr>
<tr>
<td align="center" dir="auto"><span class="emoji" title=":satellite:">📡</span></td>
<td dir="auto"><a href="station-architecture.md">How a reading travels</a></td>
<td dir="auto">Flowchart, sequence, timing and state diagrams, all written as text</td>
</tr>
<tr>
<td align="center" dir="auto"><span class="emoji" title=":electric_plug:">🔌</span></td>
<td dir="auto"><a href="tidewell-api.md">Tidewell Open Data API</a></td>
<td dir="auto">An API reference with titled code, highlighted lines and a version timeline</td>
</tr>
<tr>
<td align="center" dir="auto"><span class="emoji" title=":test_tube:">🧪</span></td>
<td dir="auto"><a href="ocean-chemistry.md">Carbon in the Sound</a></td>
<td dir="auto">Chemical equations and wave physics, typeset</td>
</tr>
<tr>
<td align="center" dir="auto"><span class="emoji" title=":bar_chart:">📊</span></td>
<td dir="auto"><a href="data-notebook.md">Ninety days of Sound data</a></td>
<td dir="auto">Charts, a flow of 155,520 readings and a wide table that scrolls</td>
</tr>
<tr>
<td align="center" dir="auto"><span class="emoji" title=":calendar:">📆</span></td>
<td dir="auto"><a href="season-plan.md">Season 3 deployment plan</a></td>
<td dir="auto">A Gantt chart, a risk quadrant, the budget and task lists</td>
</tr>
<tr>
<td align="center" dir="auto"><span class="emoji" title=":bird:">🐦</span></td>
<td dir="auto"><a href="bulletin-agent.md">Gull: the weekly bulletin agent</a></td>
<td dir="auto">A prompt file with foldable blocks and marked variables</td>
</tr>
<tr>
<td align="center" dir="auto"><span class="emoji" title=":pencil2:">✏️</span></td>
<td dir="auto"><a href="style-guide.md">How we write</a></td>
<td dir="auto">Every callout and small mark, with the source beside it</td>
</tr>
<tr>
<td align="center" dir="auto"><span class="emoji" title=":compass:">🧭</span></td>
<td dir="auto"><a href="permark-guide.md">Working in PerMark</a></td>
<td dir="auto">A ten-minute tour of reading, editing, review and sharing</td>
</tr>
<tr>
<td align="center" dir="auto"><span class="emoji" title=":stew:">🍲</span></td>
<td dir="auto"><a href="galley-recipe.md">Moqueca for twelve</a></td>
<td dir="auto">The stew that fed the crew, scaled from four servings to twelve</td>
</tr>
</tbody>
</table>
--- h2 41-41
<h2 dir="auto" id="the-crew">The crew</h2>
--- p 43-43
<p dir="auto">Five people and one patient agent keep Tidewell afloat.</p>
--- p 45-45
<p dir="auto"><img alt="Avatars of the crew: Imani, Tomás, Beatriz, Hana and Leo, plus Gull, the bulletin agent" src="pm-asset:demo/assets/crew.svg"></p>
--- dl 47-64
<dl>
<dt>Dr. Imani Okafor</dt>
<dd>Lab director and chemical oceanographer. Keeps the science honest and the <a href="style-guide.md">style guide</a> short.</dd>
<dt>Tomás Varela</dt>
<dd>Platform lead. Built the path from buoy to <a href="tidewell-api.md">API</a>, one 40-second wake at a time.</dd>
<dt>Beatriz Nogueira</dt>
<dd>Field technician and skipper of the research boat <em>Petrel</em>. Writes the <a href="field-journal.md">journal</a> and cooks the <a href="galley-recipe.md">moqueca</a>.</dd>
<dt>Hana Kowalski</dt>
<dd>Program manager and volunteer coordinator. Owns the <a href="season-plan.md">season plan</a> and the <a href="permark-guide.md">PerMark guide</a>.</dd>
<dt>Leo Park</dt>
<dd>Data scientist. Turns readings into the <a href="data-notebook.md">data notebook</a> and taught Gull to write.</dd>
<dt>Gull</dt>
<dd>The lab's read-only agent. Drafts the weekly <a href="bulletin-agent.md">bulletin</a>; a person always signs it off.</dd>
</dl>
--- h2 65-65
<h2 dir="auto" id="find-your-way">Find your way</h2>
--- p 67-67
<p dir="auto">Press <kbd>Ctrl</kbd>+<kbd>P</kbd> to jump to any page by name, or <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>F</kbd> to search all of them at once. On a long page, <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>O</kbd> opens the outline, so you can skip straight to the section you want. On macOS, press <kbd>Cmd</kbd> where you see Ctrl, and <kbd>Option</kbd> for Alt.</p>
--- p 69-69
<p dir="auto">New here? The <a href="permark-guide.md">ten-minute tour</a> covers the rest.</p>
--- hr 71-71
<hr>
--- p 73-73
<p dir="auto"><sub>Larkhaven Marine Lab, Tidewell and everyone in these pages are fictional. <a href="#top">Back to top</a></sub></p>
