## The site to build

A one-page website for **Lípa Bakery & Café**, a small family bakery in Valašské Meziříčí (Czech Republic). Use exactly the text, data and image URLs below. The site must look premium and modern (think a well-designed local business site in 2026), work on phones and desktops, and be accessible (alt texts, labels, keyboard focus).

**Look:** warm and calm. Primary color `#8A4B2A` (crust brown), accent `#E0A13A` (honey), background `#FBF7F1` (cream), text `#2B1D14`. Headings in the serif font **Fraunces**, body text in **Inter** (both from Google Fonts). Rounded cards, soft shadows, generous spacing.

**Motion:** the hero text comes in step by step (kicker, title, text, buttons); sections and their cards reveal as you scroll (cards one after another); cards lift slightly on hover; the four numbers count up when they appear; the product strip scrolls endlessly. Respect "reduced motion" settings.

### Sections, in order

1. **Navigation bar**, sticky at the top: brand "Lípa Bakery", links About (`#about`), Menu (`#menu`), Gallery (`#gallery`), Visit us (`#visit`), and a highlighted button "Order a cake" linking to `#visit`. On phones the links fold into a menu button.

2. **Hero**, full-screen photo with a dark gradient so the text is readable.
   - Photo: `https://picsum.photos/seed/lipa-hero/1600/1000` (alt: "Fresh loaves on the bakery counter")
   - Small kicker line: "Valašské Meziříčí · since 1998"
   - Title: "Bread baked before sunrise"
   - Text: "Sourdough loaves, buttery pastry and celebration cakes from a family bakery on the town square."
   - Buttons: "See the menu" (`#menu`, filled) and "Visit us" (`#visit`, outlined)
   - A small live badge "Open now" or "Closed" computed in the visitor's browser from the opening hours below (Czech time is fine to ignore: use the visitor's local time).

3. **Product strip** (endless horizontal scroll) with these words: Sourdough, Rye bread, Croissants, Kolache, Cinnamon rolls, Wedding cakes, Coffee, Seasonal tarts.

4. **About** (`#about`), kicker "Our story", title "Three generations of early mornings".
   - Paragraph: "Grandma Marie opened Lípa in 1998 with one oven and her own rye recipe. Today her grandchildren still start the ovens at 3 a.m., still use her sourdough starter, and still bake everything in the back of the shop."
   - Four numbers that count up: **28** years of baking, **40** kinds of bread and pastry, **1,200** customers a week, **3 a.m.** ovens on (the last one is text, it does not count).
   - A timeline with four steps: 1998 "Marie opens the bakery with one oven"; 2006 "A second oven and the first wedding cakes"; 2015 "The café opens next door"; 2024 "Organic flour from local mills only".

5. **Menu** (`#menu`), kicker "Menu", title "Baked today", subtitle "Prices in CZK. Ask about allergens at the counter."
   - Filter buttons: All, Bread, Pastry, Cakes. Clicking one shows only that category (without reloading the page); the active button is highlighted.
   - Nine products as cards with photo, name, short description and price. The photo URL is `https://picsum.photos/seed/lipa-N/800/600` where N is the product number:
     1. Bread · "Country sourdough" · "Long-fermented, crackling crust, open crumb." · 89 Kč
     2. Bread · "Dark rye with caraway" · "Grandma Marie's recipe, dense and fragrant." · 72 Kč
     3. Bread · "Seeded spelt loaf" · "Sunflower, pumpkin and flax seeds." · 95 Kč
     4. Pastry · "Butter croissant" · "Laminated for three days, baked every hour." · 45 Kč
     5. Pastry · "Plum kolache" · "Wallachian classic with plum jam and crumble." · 32 Kč
     6. Pastry · "Cinnamon roll" · "Soft dough, brown sugar, a little cardamom." · 49 Kč
     7. Cakes · "Honey cake" · "Ten thin layers, sour cream and honey." · 85 Kč
     8. Cakes · "Chocolate tart" · "Dark chocolate ganache on a salted crust." · 79 Kč
     9. Cakes · "Celebration cake" · "Made to order for 8–30 people. Order 3 days ahead." · from 1,200 Kč

6. **Gallery** (`#gallery`), kicker "Gallery", title "From the oven to your table". Six photos in a grid; clicking a photo opens it large (a lightbox with close and next/previous, also usable with the keyboard). Photos: `https://picsum.photos/seed/lipa-g1/1200/900` up to `lipa-g6`. Alt texts: "The bakery at dawn", "Shaping sourdough loaves", "Croissants fresh from the oven", "The café corner", "A wedding cake on the counter", "Flour sacks from the local mill".

7. **Testimonials**, title "What our neighbours say", three quotes:
   - "The rye bread tastes exactly like my grandmother's. I come every Saturday." — Jana K.
   - "They made our wedding cake in three days and it was perfect." — Tomáš and Eva
   - "Best croissant in Wallachia, and the coffee is great too." — Petr S.

8. **Visit us** (`#visit`), kicker "Visit us", title "Come in for a coffee".
   - Opening hours: Monday–Friday 6:00–18:00, Saturday 7:00–12:00, Sunday closed; today's row highlighted, with the same live "Open now"/"Closed" badge.
   - Address: "Náměstí 12, 757 01 Valašské Meziříčí", with a link "Open in Google Maps" (`https://maps.google.com/?q=Náměstí+12,+Valašské+Meziříčí`).
   - Phone "+420 571 000 123" (clickable `tel:` link) and e-mail "hello@lipa-bakery.cz" (clickable `mailto:` link).
   - A contact form with Name (required), E-mail (required), Message (required). Sending it opens the visitor's e-mail app with the message filled in, addressed to hello@lipa-bakery.cz (no server).

9. **Footer**: "Lípa Bakery & Café", the address, the four section links, and "© 2026 Lípa Bakery & Café".

Page title: "Lípa Bakery & Café – bread baked before sunrise, Valašské Meziříčí". Description for search engines: "Family bakery and café on the town square in Valašské Meziříčí since 1998: sourdough bread, pastry and celebration cakes."
