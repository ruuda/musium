output/Main/index.js: src/Main.purs
	psc-package build

output/main.js: output/Main/index.js
	purs bundle output/*/*.js -m Main > output/main.js
