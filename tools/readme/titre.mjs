// Dessine le titre de la page d'accueil du dépôt en matrice de points, comme la police Doto de l'app.
// Aucune dépendance. Usage : node tools/readme/titre.mjs   → écrit docs/readme/titre.svg
//
// GitHub retire les balises <style> d'un README mais garde celles qui sont DANS un fichier SVG affiché par
// <img> : c'est pourquoi l'animation et le thème clair / sombre vivent ici, pas dans le Markdown.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const GLYPHES = {
  S: ['01111', '10000', '10000', '01110', '00001', '00001', '11110'],
  P: ['11110', '10001', '10001', '11110', '10000', '10000', '10000'],
  A: ['01110', '10001', '10001', '11111', '10001', '10001', '10001'],
  C: ['01111', '10000', '10000', '10000', '10000', '10000', '01111'],
  E: ['11111', '10000', '10000', '11110', '10000', '10000', '11111'],
  F: ['11111', '10000', '10000', '11110', '10000', '10000', '10000'],
  I: ['111', '010', '010', '010', '010', '010', '111'],
  L: ['10000', '10000', '10000', '10000', '10000', '10000', '11111'],
};

const MOT = 'SPACEFILES';
const PAS = 14; // distance entre deux points
const RAYON = 4.6;
const MARGE = 28;
const LIGNES = 7;

// Colonnes allumées du mot, une colonne vide entre deux lettres
const colonnes = [];
for (const lettre of MOT) {
  const g = GLYPHES[lettre];
  for (let x = 0; x < g[0].length; x++) colonnes.push(g.map((ligne) => ligne[x] === '1'));
  colonnes.push(new Array(LIGNES).fill(false));
}
colonnes.pop();

const largeurMot = colonnes.length * PAS;
const largeur = largeurMot + MARGE * 2;
const yJauge = MARGE + LIGNES * PAS + 30;
const hauteur = yJauge + 58;

let points = '';
colonnes.forEach((col, x) => {
  col.forEach((allume, y) => {
    const cx = MARGE + x * PAS + PAS / 2;
    const cy = MARGE + y * PAS + PAS / 2;
    // Les points s'allument de gauche à droite, comme un dossier qui se liste
    const retard = (x * 0.028 + y * 0.012).toFixed(3);
    points += allume
      ? `<circle class="on" cx="${cx}" cy="${cy}" r="${RAYON}" style="animation-delay:${retard}s"/>`
      : `<circle class="off" cx="${cx}" cy="${cy}" r="${RAYON}"/>`;
  });
});

// Jauge d'opération de l'app : 16 barres qui se remplissent, puis la pastille de fin
const BARRES = 16;
let jauge = '';
for (let i = 0; i < BARRES; i++) {
  jauge += `<rect class="barre" x="${MARGE + i * 11}" y="${yJauge}" width="6" height="18" rx="1" style="animation-delay:${(1.6 + i * 0.09).toFixed(2)}s"/>`;
}

// Pixel : deux barres qui clignent, à droite. Ce n'est pas une mascotte, c'est un voyant.
const xPixel = largeur - MARGE - 22;
const pixel =
  `<g class="pixel"><rect x="${xPixel}" y="${yJauge}" width="6" height="18" rx="1"/>` +
  `<rect x="${xPixel + 14}" y="${yJauge}" width="6" height="18" rx="1"/></g>`;

const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${largeur} ${hauteur}" width="${largeur}" height="${hauteur}" role="img" aria-label="SpaceFiles">
<title>SpaceFiles</title>
<style>
  :root { --fond:#000; --eteint:#1c1c1c; --allume:#f2f2f2; --accent:#E8A66B; --texte:#8a8a8a; }
  @media (prefers-color-scheme: light) {
    :root { --fond:#f4f2ee; --eteint:#dedbd5; --allume:#111; --accent:#b8692a; --texte:#6b6b6b; }
  }
  .fond { fill:var(--fond); }
  .off { fill:var(--eteint); }
  .on { fill:var(--allume); opacity:0; animation:allumer .35s ease-out forwards; }
  .barre { fill:var(--eteint); animation:remplir .2s linear forwards; }
  .pixel rect { fill:var(--accent); transform-box:fill-box; transform-origin:center; animation:cligner 5s infinite; }
  .etat { fill:var(--texte); font:600 12px ui-monospace,Consolas,monospace; letter-spacing:2px; }
  .fin { opacity:0; animation:allumer .4s ease-out 3.2s forwards; }
  @keyframes allumer { from { opacity:0; fill:var(--accent); } to { opacity:1; } }
  @keyframes remplir { to { fill:var(--accent); } }
  @keyframes cligner { 0%,92%,100% { transform:scaleY(1); } 95% { transform:scaleY(.12); } }
  @media (prefers-reduced-motion: reduce) {
    .on, .fin { animation:none; opacity:1; }
    .barre { animation:none; fill:var(--accent); }
    .pixel rect { animation:none; }
  }
</style>
<rect class="fond" width="${largeur}" height="${hauteur}" rx="14"/>
${points}
${jauge}
<text class="etat fin" x="${MARGE + BARRES * 11 + 12}" y="${yJauge + 14}">FILE EXPLORER · WINDOWS 11 · RUST</text>
${pixel}
</svg>
`;

const sortie = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'docs', 'readme', 'titre.svg');
fs.mkdirSync(path.dirname(sortie), { recursive: true });
fs.writeFileSync(sortie, svg);
console.log(`${path.relative(process.cwd(), sortie)} écrit : ${largeur}×${hauteur}, ${colonnes.length} colonnes, ${svg.length} octets`);
