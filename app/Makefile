output/app.js: output/Main/index.js
	spago bundle-app --minify --to output/app.js

output/Main/index.js: src/*.purs src/*.js spago.dhall packages.dhall
	spago build
