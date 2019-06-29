output/main.js: output/Main/index.js
	purs bundle output/*/*.js -m Main > output/main.js

output/Main/index.js: src/*.purs psc-package.json
	psc-package build
