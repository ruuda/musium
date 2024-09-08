output/app.js: output/Main/index.js
	spago bundle --minify --outfile output/app.js

output/Main/index.js: src/*.purs src/*.js spago.lock spago.yaml
	spago build
