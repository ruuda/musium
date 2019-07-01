output/app.js: output/Main/index.js
	purs bundle output/*/*.js -m Main > output/app.js

output/Main/index.js: src/*.purs src/*.js psc-package.json
	psc-package build
