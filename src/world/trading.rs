//! What a building is allowed to call itself.
//!
//! The atlas is a faithful OpenStreetMap extract and the map names the real
//! shops: `--rename` took the buildings dump's `name` tag precisely so the
//! Finanzamt and the Galeria would wear their own. That is right for a
//! Finanzamt and wrong for a Galeria. A comedy game with no fail state and a
//! rubber population should not put a living company's sign over a door, and
//! it certainly should not put a private person's name on a building, which
//! one of these was.
//!
//! So the atlas keeps saying what the map says — it is the ODbL extract and it
//! stays honest — and this is the one place between the file and the player
//! where a name becomes something the game says out loud. Two rules:
//!
//! * The town is Landshüpf. Anything that calls it Landshut is corrected, and
//!   that alone covers fourteen of the town's institutions, from the
//!   Volkshochschule to the LANDSHUTmuseum.
//! * A living trading name, or a person's, is replaced outright by [`RENAMED`].
//!
//! [`KEPT`] is the other half of the same decision, and it is why both lists
//! are written out in full: `every_name_in_the_atlas_has_been_looked_at` holds
//! their union against the committed file exactly, so a re-bake that brings a
//! new business into town fails a test until somebody has decided what the
//! game calls it. A list that only names the problems cannot do that — it
//! would pass on the day a McDonald\'s appeared.
//!
//! The civic and the historic keep their names, and that is the point of
//! reading a real map: the Stadtresidenz is the Stadtresidenz, St. Martin is
//! St. Martin, and the Dürnitztrakt is a wing of a castle rather than a brand.

use std::borrow::Cow;

/// The town on the map, and the town in the game.
const REAL_TOWN: &str = "Landshut";
const OUR_TOWN: &str = "Landshüpf";
/// The same pair as the museum shouts it.
const REAL_TOWN_LOUD: &str = "LANDSHUT";
const OUR_TOWN_LOUD: &str = "LANDSHÜPF";

/// What the game calls a building the map has named.
///
/// Borrowed wherever nothing had to change, which is most of the time: this
/// runs once per named building at world build, and a town has fewer than two
/// hundred of them.
pub fn traded(name: &str) -> Cow<'_, str> {
    if let Some((_, ours)) = RENAMED.iter().find(|(theirs, _)| *theirs == name) {
        return Cow::Borrowed(*ours);
    }
    if name.contains(REAL_TOWN_LOUD) {
        return Cow::Owned(name.replace(REAL_TOWN_LOUD, OUR_TOWN_LOUD));
    }
    if name.contains(REAL_TOWN) {
        return Cow::Owned(name.replace(REAL_TOWN, OUR_TOWN));
    }
    Cow::Borrowed(name)
}

/// Every living trading name in the committed atlas, and what stands in for
/// it. Left is what the map says; right is what the town says.
///
/// Sorted by what the map says, so a new entry has one obvious place to go.
pub const RENAMED: &[(&str, &str)] = &[
    ("AOK", "AUA"),
    ("AWO", "AHA"),
    ("Adana Kebap", "Kebap Kaboom"),
    ("Agip", "Agipf"),
    ("Beauty Studio Mel", "Schönheitsstudio Molli"),
    ("Blumen Brunner", "Blumen Brummer"),
    ("Brotmacher", "Brotwerfer"),
    ("C&A", "Hemd & Hose"),
    ("Campus City", "Campus Hupf"),
    ("CarWash", "Wagenwäsche"),
    ("Carglass", "Glasklar Sofort"),
    ("City Center Landshut", "City Center Landshüpf"),
    ("City Hotel Isar-Residenz", "City Hotel Isar-Resonanz"),
    (
        "Deutsche Rentenversicherung Bayern Süd",
        "Deutsche Federversicherung Bayern Süd",
    ),
    ("Edeka", "Immersatt"),
    ("El Cubanito", "El Flummito"),
    ("Figaro", "Figarotz"),
    ("Frisch und Fein", "Frisch und Fedrig"),
    ("Galeria", "Galoschia"),
    ("Gärtnerei Wachtler", "Gärtnerei Wackler"),
    (
        "HAARMONIE - Friseursalon für SIE und IHN",
        "HAARMONIKA - Friseursalon für SIE und IHN",
    ),
    ("Hotel Luitpold", "Hotel Luitgold"),
    ("Hotel Park-Cafe", "Hotel Park-Kaffee"),
    ("Juwelier Stanglmayr", "Juwelier Stanglhupf"),
    ("Lebenshilfe", "Lebenshupf"),
    ("Lipp", "Lupp"),
    ("Mayrhofer", "Maierhupfer"),
    ("Moserbräu", "Hoserbräu"),
    ("New mountains", "Neue Berge"),
    ("Oberpaur", "Oberprall"),
    ("P2/8 Karstadt & Oberpaur", "P2/8 Kaufstatt & Oberprall"),
    ("Penzkofer Reklamewerkstatt", "Prellkofer Reklamewerkstatt"),
    ("Poseidon BAR", "Poseidonner BAR"),
    ("Reiskorn", "Reishüpfer"),
    ("Remzi Cakmak", "Imbiss am Eck"),
    ("Rewe", "Rundum"),
    (
        "Ristorante-Pizzeria buono",
        "Ristorante-Pizzeria Bello Boing",
    ),
    ("Salon Diamond", "Salon Diamantig"),
    ("Sparda-Bank", "Sparwoanders-Bank"),
    ("Sparkasse Landshut", "Sparbüchse Landshüpf"),
    ("Sport Schäbel", "Sport Schnäbel"),
    ("Sterncenter", "Sternhupf-Center"),
    ("VR-Bank Landshut Arena", "Hupf-Bank Landshüpf Arena"),
    ("Waldorfkindergarten", "Waldhupf-Kindergarten"),
    (
        "aiutanda ambulanter Pflegedienst Landshut",
        "Ambulanter Pflegedienst Landshüpf",
    ),
];

/// Every name the game says as the map says it: the civic, the historic, the
/// ecclesiastical and the topographic. Held as a list rather than as a rule
/// because there is no rule — `AWO` is a `TownHall` in this atlas and
/// `Sparda-Bank` has no kind at all, so nothing about a building says whether
/// its name belongs to a company.
pub const KEPT: &[&str] = &[
    "Afrakapelle",
    "Agentur für Arbeit",
    "Alt St. Nikola",
    "Alte Post",
    "Alter Bahnhof",
    "Altes Franziskanerkloster",
    "Amtsgericht Landshut",
    "Aussegnungshalle",
    "Banane",
    "Basilika Sankt Martin",
    "Bau D",
    "Bau F",
    "Bau K",
    "Bauzunfthaus",
    "Bayerisches Staatsarchiv",
    "Berg",
    "Besucherparkplatz Klinikum",
    "Burghauser Tor",
    "Christuskirche",
    "Container (HCG)",
    "Damenstock",
    "Dominikanerkirche St. Blasius",
    "Dultwachgebäude 1",
    "Dultwachgebäude 2",
    "Dürnitztrakt",
    "Ehem. Maschinenbaufachschule",
    "Falkenturm",
    "Faltboot-Klub Landshut e.V. (FKL)",
    "Finanzamt Landshut",
    "Fortluftzentrale Hofgartenparkplatz",
    "Frauenkapelle",
    "Freiwillige Feuerwehr Landshut (LZ 5 - Hofberg)",
    "Freundschaftstempel",
    "Fürstenbau",
    "Gerichtsdienerhaus",
    "Gewerbe-Haus",
    "Hallenbad Stadtbad Landshut",
    "Hauptgebäude (HCG)",
    "Heilig Geist",
    "Heilig-Geist-Spital Alten- und Pflegeheim",
    "Herzogschlößl",
    "Hofberg Villa",
    "Hofgärtnerhaus",
    "Hofstallgebäude",
    "Hungerturm",
    "Inneres Torwarthaus",
    "Italienischer Anbau",
    "Jesuitenkirche Sankt Ignatius",
    "Jägerhaus",
    "Kellereigebäude",
    "Kindergarten St. Konrad",
    "Kinderkrankenhaus Sankt Marien",
    "Königmuseum im Hofberg",
    "LANDSHUTmuseum",
    "Landesamt für Finanzen",
    "Landgericht Landshut",
    "Ländtor",
    "Magdalenenheim",
    "Marstall",
    "Matthäusstift",
    "Maxwehr",
    "Modulschule Ursulinen Realschule",
    "Münzturm",
    "Nebengebäude (HCG)",
    "Nebengebäude Gymnasium Seligenthal",
    "Neuapostolische Kirche",
    "Nikolausheim",
    "Ottonianum",
    "P3 An Der Freyung",
    "P6 Zentrum",
    "Pappenbergerhaus",
    "Parkhaus Kinderkrankenhaus Sankt Marien",
    "Pavillion",
    "Pfaffenstöckl",
    "Pfarrheim Sankt Martin",
    "Pfarrheim St. Nikola",
    "Pulverturm",
    "Rathaus II",
    "Realschulgebäude (HCG)",
    "Regierung von Niederbayern",
    "Rochuskapelle",
    "Ruheraum",
    "Rumänisch-Orthodoxe Kirche Johannes der Wallache",
    "Salzstadl",
    "Sankt Pius",
    "Sausteg",
    "Schlosspflegerhaus",
    "Schwedentor",
    "Seniorenheim St.-Jodok-Stift",
    "Sozialgericht Landshut, Arbeitsgericht Regensburg",
    "Spielkasino Röcklturm",
    "Sportzentrum West",
    "St Sebastian",
    "St. Jodok",
    "St. Konrad",
    "St. Nikola",
    "St. Nikolaus",
    "Staatliche Berufsoberschule Landshut",
    "Staatsanwaltschaft Landshut",
    "Stadtresidenz",
    "Stadtwerke Landshut",
    "Sternwarte Seligenthal",
    "Telefonladen Landshut",
    "Theklakapelle",
    "Torhaus",
    "Tunnelhaus",
    "Tunnelportal Ost",
    "Tunnelportal West",
    "Turnhalle (HCG)",
    "Turnhalle Ursulinen-Realschule",
    "Turnhallen Seligenthal",
    "Umkleiden Freibad",
    "Ursulinenkirche",
    "Ussar Villa",
    "Volkshochschule Landshut",
    "Waffenturm",
    "Wartturm",
    "Wasserturm",
    "Wasserwirtschaftsamt Landshut",
    "Wintergarten",
    "Wittelsbacherturm",
    "Zentrum Bayern Familie und Soziales",
    "Zeughaus",
    "ehem. Martinsschule",
    "ehemaliges Ursulinenkloster Sankt Joseph",
    "Äußeres Torwarthaus",
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// The two registers together are the committed atlas, exactly.
    ///
    /// Both directions matter. A name in the atlas and in neither list is one
    /// nobody has decided about, which is how a real company gets a shopfront
    /// in a comedy game. A name in a list and not in the atlas is a decision
    /// about a building that no longer exists, and it is the first thing to go
    /// stale after a re-bake.
    #[test]
    fn every_name_in_the_atlas_has_been_looked_at() {
        let Some(town) = crate::world::atlas::load("landshut") else {
            assert!(
                !crate::core::assets::root()
                    .join("cities/landshut.ron")
                    .exists(),
                "the committed Landshut does not load as an atlas"
            );
            return;
        };
        let in_atlas: BTreeSet<&str> = town
            .buildings
            .iter()
            .map(|footprint| footprint.name.as_str())
            .filter(|name| !name.is_empty())
            .collect();
        let decided: BTreeSet<&str> = RENAMED
            .iter()
            .map(|(theirs, _)| *theirs)
            .chain(KEPT.iter().copied())
            .collect();
        let undecided: Vec<&&str> = in_atlas.difference(&decided).collect();
        assert!(
            undecided.is_empty(),
            "the atlas names buildings nobody has decided about: {undecided:?}"
        );
        let stale: Vec<&&str> = decided.difference(&in_atlas).collect();
        assert!(
            stale.is_empty(),
            "the registers name buildings the atlas does not: {stale:?}"
        );
    }

    /// And nothing a living company owns reaches a sign.
    #[test]
    fn no_trading_name_survives_the_walk() {
        let Some(town) = crate::world::atlas::load("landshut") else {
            return;
        };
        for footprint in &town.buildings {
            if footprint.name.is_empty() {
                continue;
            }
            let shown = traded(&footprint.name);
            assert!(
                !RENAMED.iter().any(|(theirs, _)| *theirs == shown),
                "{shown} is what the map calls a real business"
            );
            assert!(
                !shown.contains(REAL_TOWN) && !shown.contains(REAL_TOWN_LOUD),
                "{shown} still names the real town"
            );
        }
    }

    #[test]
    fn the_town_is_renamed_wherever_it_is_mentioned() {
        assert_eq!(
            traded("Volkshochschule Landshut"),
            "Volkshochschule Landshüpf"
        );
        assert_eq!(traded("LANDSHUTmuseum"), "LANDSHÜPFmuseum");
        assert_eq!(traded("Stadtresidenz"), "Stadtresidenz");
    }

    /// A register entry wins over the town rule, because it already applied it.
    #[test]
    fn a_register_entry_is_taken_whole() {
        assert_eq!(traded("Sparkasse Landshut"), "Sparbüchse Landshüpf");
        assert_eq!(traded("Sparda-Bank"), "Sparwoanders-Bank");
    }
}
