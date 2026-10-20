You are a senior front-end developer. Build the website described below as **one single file, `site.html`**, that works when opened directly in a browser.

## Rules
- Everything in one file: HTML, all CSS in `<style>`, all JavaScript in `<script>`. No build step.
- No frameworks or libraries (no React, Tailwind, jQuery, animation libraries or icon fonts). Plain HTML, CSS and JavaScript only. Google Fonts is the only external stylesheet allowed; icons, if you want them, are inline SVG.
- The images are the given URLs; do not use other images.
- It must work: no console errors, every interaction in the brief works.

## The site to build

A one-page website for **Penzion U Mlýna**, a family-run guesthouse with six rooms in an old mill on the Vltava river in Český Krumlov (Czech Republic). Use exactly the text, data and image URLs below. The site must look premium, warm and welcoming (think a well-designed boutique hotel site in 2026), work on phones and desktops, and be accessible (alt texts, labels, keyboard focus).

**Look:** warm and natural. Background `#FBF8F3` (warm off-white), surfaces `#FFFFFF`, text `#2B2622`, primary color `#7A4E2D` (walnut brown), accent `#5B7F5E` (sage green). Headings in **Fraunces**, body text in **Nunito Sans** (both from Google Fonts). Rounded cards, soft shadows, photos with rounded corners, lots of space.

**Motion:** the hero text comes in step by step (kicker, title, text, buttons); sections and their cards reveal as you scroll (cards one after another); room cards and gallery photos zoom in slightly on hover. Respect "reduced motion" settings: with reduced motion nothing moves.

### Sections, in order

1. **Navigation bar**, sticky at the top: brand "U Mlýna", links Rooms (`#rooms`), Amenities (`#amenities`), Gallery (`#gallery`), Location (`#location`), FAQ (`#faq`), and a highlighted button "Book a stay" linking to `#booking`. On phones the links fold into a menu button.

2. **Hero**, a full-screen photo with a dark gradient so the text is readable.
   - Photo: `https://picsum.photos/seed/mlyn-hero/1600/1000` (alt: "The old mill house of Penzion U Mlýna on the Vltava river")
   - Small kicker line: "Český Krumlov · on the Vltava since 1998"
   - Title: "Sleep by the river, wake up to the castle"
   - Text: "Six quiet rooms in a restored 17th-century mill, five minutes' walk from the old town. Breakfast from our own bakery included."
   - Buttons: "See the rooms" (`#rooms`, filled) and "Book a stay" (`#booking`, outlined)
   - A small live badge that says exactly "Reception open" or "Reception closed", computed in the visitor's browser from the reception hours below (use the visitor's local time).

3. **Rooms** (`#rooms`), kicker "Rooms", title "Find your room", subtitle "Every room has a private bathroom, free Wi-Fi and breakfast included. Prices are per night for the whole room."
   - Filter buttons: All, Double, Family, Suite. "All" is active when the page loads. Clicking a type shows only the rooms of that type (without reloading the page); the active button is highlighted.
   - Six rooms as cards, each with a photo, the name, the type, the size, the number of guests, the price per night and one sentence:
     1. "River Room" · Double · 18 m² · 2 guests · 2,400 Kč per night · "A window seat right above the weir." · photo `https://picsum.photos/seed/mlyn-room-1/800/600` (alt: "The River Room with a window seat")
     2. "Garden Room" · Double · 16 m² · 2 guests · 2,100 Kč per night · "Opens onto the herb garden and is quiet at night." · photo `https://picsum.photos/seed/mlyn-room-2/800/600` (alt: "The Garden Room with a view of the herb garden")
     3. "Miller's Family Room" · Family · 32 m² · 4 guests · 3,600 Kč per night · "A double bed and a bunk bed under old oak beams." · photo `https://picsum.photos/seed/mlyn-room-3/800/600` (alt: "The family room with a bunk bed")
     4. "Attic Family Room" · Family · 36 m² · 5 guests · 3,900 Kč per night · "Two bedrooms under the roof, ideal with older kids." · photo `https://picsum.photos/seed/mlyn-room-4/800/600` (alt: "The attic family room under the roof")
     5. "Castle View Suite" · Suite · 45 m² · 2 guests · 5,200 Kč per night · "A free-standing bath and the best view of the castle tower." · photo `https://picsum.photos/seed/mlyn-room-5/800/600` (alt: "The Castle View Suite with a free-standing bath")
     6. "Mill Wheel Suite" · Suite · 50 m² · 4 guests · 5,900 Kč per night · "A living room with the original mill wheel behind glass." · photo `https://picsum.photos/seed/mlyn-room-6/800/600` (alt: "The Mill Wheel Suite with the old mill wheel")

4. **Amenities** (`#amenities`), kicker "Amenities", title "Everything for a slow weekend". Eight items, each with a small icon and a label: "Breakfast from our bakery", "Free Wi-Fi", "Free private parking", "Riverside garden terrace", "Bike and canoe storage", "Pets welcome (300 Kč per night)", "Sauna in the old cellar", "Early check-in on request".

5. **Gallery** (`#gallery`), kicker "Gallery", title "A look around". Six photos in a grid; clicking a photo opens it large (a lightbox with a close button and next/previous buttons, also usable with the keyboard: Escape closes, arrow keys move). Photos: `https://picsum.photos/seed/mlyn-g1/1200/800` up to `mlyn-g6`. Alt texts: "The mill house at dusk", "Breakfast on the terrace", "The weir below the River Room", "The reading corner by the fireplace", "Canoes on the riverbank", "The castle tower from our garden".

6. **Reviews**, title "What our guests say", three reviews, each with five stars:
   - "The quietest night we have had in years, and the croissants alone are worth the trip." — Anna & Tom, United Kingdom
   - "Our kids loved the bunk bed and the ducks on the river." — The Huber family, Austria
   - "Five minutes from the old town, yet it feels like the countryside." — Petr K., Prague

7. **Book a stay** (`#booking`), kicker "Book a stay", title "Ask for your dates", text "Tell us when you would like to come. We reply within a few hours with availability and the price."
   - A form with Arrival (required, a date field), Departure (required, a date field), Guests (required, a select with: 1, 2, 3, 4, 5), Room type (required, a select with: Double, Family, Suite, Any), Name (required), E-mail (required), Phone (optional) and Message (optional). Every field has a visible label. The button says "Check availability".
   - Sending it opens the visitor's e-mail app with a message addressed to stay@umlyna.cz, subject "Booking request", and all the entered details in the body (no server).

8. **Location** (`#location`), kicker "Location", title "Five minutes from the old town".
   - Address: "Linecká 12, 381 01 Český Krumlov", with a link "Open in Google Maps" (`https://maps.google.com/?q=Linecká+12,+Český+Krumlov`).
   - How far things are: Old town square · 5 min on foot; Castle · 10 min on foot; Train station · free pick-up on request.
   - Phone "+420 380 000 512" (clickable `tel:` link) and e-mail "stay@umlyna.cz" (clickable `mailto:` link).
   - Reception hours: Monday–Friday 7:00–21:00, Saturday–Sunday 8:00–20:00, with the same live "Reception open"/"Reception closed" badge. Below them: "Check-in from 14:00, check-out until 11:00."

9. **FAQ** (`#faq`), kicker "FAQ", title "Good to know". Clicking a question shows its answer; clicking it again hides it. Four questions:
   - "Is breakfast included?" · "Yes. Breakfast is served from 7:30 to 10:00 in the mill room or on the terrace."
   - "Can I arrive late in the evening?" · "Yes. Tell us your arrival time and we will leave your key in a code box at the door."
   - "Do you have parking?" · "We have eight free spaces in our courtyard. There is no need to book one."
   - "Can I cancel my booking?" · "Free cancellation up to 7 days before arrival. After that we charge the first night."

10. **Footer**: "Penzion U Mlýna", the address, the five section links, and "© 2026 Penzion U Mlýna".

Page title: "Penzion U Mlýna – riverside guesthouse in Český Krumlov". Description for search engines: "Family-run guesthouse in a restored mill on the Vltava, five minutes from the old town of Český Krumlov: six rooms, breakfast included, free parking."

## Your answer
Reply with the complete `site.html` in one code block and nothing else: no explanation before or after, no "rest of the code here" placeholders.
