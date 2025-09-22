use super::regex_val_gen::RegexValGen;
use chrono::{Datelike, Duration, Utc};
use quote::{ToTokens, quote};
use regex::Regex;
use tracing;
use try_from_expr::TryFromExpr;

/// Generate a regex pattern for dates within a specified number of days from now
fn generate_date_range_pattern(days: i64) -> String {
    tracing::trace!(days = days, "Generating date range pattern");
    let now = Utc::now();

    // Collect all valid dates in the range
    let mut date_patterns = Vec::new();
    for i in 0..=days {
        let date = now + Duration::days(i);
        date_patterns.push(format!(
            "{:04}-{:02}-{:02}",
            date.year(),
            date.month(),
            date.day()
        ));
    }

    // Return the joined pattern
    let pattern = format!("({})", date_patterns.join("|"));
    tracing::trace!(
        pattern_count = date_patterns.len(),
        "Date range pattern generated"
    );
    pattern
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, TryFromExpr, serde::Serialize, serde::Deserialize)]
pub enum Format {
    /// Generate a random UUID string
    Uuid,
    /// Generate a random datetime string (ISO 8601 format)
    DateTime,
    /// Generate a random date string (YYYY-MM-DD format)
    Date,
    /// Generate a random time string (HH:MM:SS format)
    Time,
    /// Generate a random hex string of specified length
    HexString(usize),
    /// Generate a random base64 string
    Base64String(usize),
    /// Generate a random JSON Web Token (JWT) format string
    JwtToken,
    /// Generate a random credit card number (test format)
    CreditCardNumber,
    /// Generate a random US Social Security Number (test format)
    SocialSecurityNumber,
    /// Generate a random IP address
    IpAddress,
    /// Generate a random MAC address
    MacAddress,
    /// Generate a random color hex code (e.g., #FF5733)
    ColorHex,
    /// Generate a random Oklch color (e.g., oklch(70% 0.3 60))
    Oklch,
    /// Generate a random filename with extension
    Filename(String), // extension
    /// Generate a random URL with specified domain
    Url(String), // domain
    /// Generate a random currency amount (formatted as string)
    CurrencyAmount,
    /// Generate a random percentage (0-100)
    Percentage,
    /// Generate a random latitude coordinate
    Latitude,
    /// Generate a random longitude coordinate
    Longitude,
    /// Generate a random company name
    CompanyName,
    /// Generate a random job title
    JobTitle,
    /// Generate a random street address
    StreetAddress,
    /// Generate a random city name
    City,
    /// Generate a random state/province name
    State,
    /// Generate a random postal/zip code
    PostalCode,
    /// Generate a random country name
    Country,
    /// Generate a random lorem ipsum text of specified word count
    LoremIpsum(usize),
    /// Generate a random product name
    ProductName,
    /// Generate a random SKU/product code
    ProductSku,
    /// Generate a random version string (e.g., "1.2.3")
    Version,
    /// Generate a random hash string (SHA-256 format)
    Hash,
    /// Generate a random user agent string
    UserAgent,
    /// Generate a random email address
    Email,
    /// Generate a random first name
    FirstName,
    /// Generate a random last name
    LastName,
    /// Generate a random full name
    FullName,
    /// Generate a random phone number
    PhoneNumber,
    /// Generate a random ISO8601 duration string (e.g., "PT1H30M")
    Iso8601DurationString,
    /// Generate a random timezone identifier (e.g., "America/New_York")
    TimeZone,
    /// Generate a random date within the next N days, rounded to 15-minute intervals
    DateWithinDays(i64),
    /// Generate an appointment datetime within the next 10 days, 7am-8pm, 15-minute intervals
    AppointmentDateTime,
    /// Generate a set of Tailwind-style colors (main, hover, active)
    TailwindColorSet(Option<String>), // Optional color name for seeding
    /// Custom format with a user-provided pattern
    Custom(String),
    /// Generate a completely random string of 8-16 characters
    Random,
    /// Generate appointment duration in nanoseconds (1-5 hours in 15-minute increments)
    AppointmentDurationNs,
}

impl Format {
    /// Helper function to generate a value from regex pattern
    fn generate_from_regex(&self) -> String {
        tracing::trace!(format = ?self, "Generating value from regex pattern");
        let regex: Regex = self.clone().into();
        let pattern = regex.as_str();

        let mut maker = RegexValGen::new();

        let result = maker
            .generate(pattern)
            .unwrap_or_else(|e| panic!("Failed to generate value for {:?}: {}", self, e));

        tracing::trace!(value_length = result.len(), "Generated value from regex");
        result
    }

    pub fn generate_formatted_value(&self) -> String {
        tracing::debug!(format = ?self, "Generating formatted value");
        self.generate_from_regex()
    }

    /// Convert this Format into a Regex
    pub fn into_regex(self) -> Regex {
        tracing::trace!(format = ?self, "Converting format to regex");
        self.into()
    }
}

impl From<Format> for Regex {
    fn from(format: Format) -> Self {
        tracing::trace!(format = ?format, "Creating regex from format");
        let pattern = match format {
            Format::Uuid => {
                r"^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$"
            }
            Format::DateTime => {
                r"^(202[0-9])-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])T([01][0-9]|2[0-3]):(0|15|30|45):[0-5][0-9]Z$"
            }
            Format::Date => r"^(202[0-9])-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])$",
            Format::Time => r"^\d{2}:\d{2}:\d{2}$",
            Format::HexString(len) => {
                return Regex::new(&format!(r"^[0-9a-fA-F]{{{}}}$", len))
                    .expect("Failed to create hex string regex");
            }
            Format::Base64String(len) => {
                return Regex::new(&format!(r"^[A-Za-z0-9+/]{{{}}}$", len))
                    .expect("Failed to create base64 string regex");
            }
            Format::JwtToken => r"^[A-Za-z0-9+/]{36}\.[A-Za-z0-9+/]{36}\.[A-Za-z0-9+/]{43}$",
            Format::CreditCardNumber => r"^\d{4}-\d{4}-\d{4}-\d{4}$",
            Format::SocialSecurityNumber => r"^\d{3}-\d{2}-\d{4}$",
            Format::IpAddress => {
                r"^(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)$"
            }
            Format::MacAddress => r"^([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}$",
            Format::ColorHex => r"^#[0-9a-fA-F]{6}$",
            Format::Oklch => r"^oklch\(\d+(?:\.\d+)?% \d+(?:\.\d+)? \d+(?:\.\d+)?\)$",
            Format::Filename(ref extension) => {
                return Regex::new(&format!(r"^[a-z]{{8}}\.{}$", regex::escape(extension)))
                    .expect("Failed to create filename regex");
            }
            Format::Url(ref domain) => {
                return Regex::new(&format!(r"^https://{}/[a-z]{{8}}$", regex::escape(domain)))
                    .expect("Failed to create URL regex");
            }
            Format::CurrencyAmount => r"^\$\d+\.\d{2}$",
            Format::Percentage => r"^\d+(?:\.\d+)?%$",
            Format::Latitude => r"^-?\d+\.\d{6}$",
            Format::Longitude => r"^-?\d+\.\d{6}$",
            Format::CompanyName => {
                r"^(Apple|Google|Microsoft|Amazon|Facebook|Tesla|Netflix|Adobe|Oracle|Intel|IBM|Cisco|Salesforce|PayPal|Spotify|Uber|Airbnb|Twitter|LinkedIn|Zoom|Slack|GitHub|Docker|Stripe|Square|Dropbox|Reddit|Pinterest|Snapchat|TikTok|Twitch|Discord|Shopify|Cloudflare|DataDog|MongoDB|Elastic|HashiCorp|GitLab|Atlassian|JetBrains|Unity|Epic Games|Valve|OpenAI|DeepMind|Anthropic|Palantir|SpaceX|Blue Origin|Boeing|Lockheed Martin|Raytheon|Northrop Grumman|General Dynamics|Honeywell|3M|Johnson & Johnson|Pfizer|Moderna|AstraZeneca|Merck|Abbott|Medtronic|Boston Scientific|Nike|Adidas|Under Armour|Lululemon|Patagonia|North Face|Columbia|REI|Walmart|Target|Costco|Home Depot|Lowes|Best Buy|GameStop|Barnes & Noble|Starbucks|McDonalds|Subway|Chipotle|Dominos|Pizza Hut|KFC|Burger King|Wendys|Dunkin|Tim Hortons|JP Morgan|Goldman Sachs|Morgan Stanley|Bank of America|Wells Fargo|Citibank|American Express|Visa|Mastercard|Discover|Capital One|Charles Schwab|Fidelity|Vanguard|BlackRock|State Street|BNY Mellon|Northern Trust|Ford|General Motors|Toyota|Honda|Tesla|Rivian|Lucid|Volkswagen|BMW|Mercedes|Audi|Porsche|Ferrari|Lamborghini|McLaren|Rolls Royce|Bentley|Aston Martin|Jaguar|Land Rover|Volvo|Mazda|Subaru|Mitsubishi|Nissan|Hyundai|Kia|Genesis|Polestar|ExxonMobil|Chevron|Shell|BP|ConocoPhillips|Marathon|Valero|Phillips 66|Occidental|Halliburton|Schlumberger|Baker Hughes|AT&T|Verizon|T-Mobile|Sprint|Comcast|Charter|Cox|CenturyLink|Frontier|Windstream|Dish|DirecTV|Coca Cola|Pepsi|Dr Pepper|Monster|Red Bull|Gatorade|Powerade|Vitamin Water|Nestle|Unilever|Procter & Gamble|Colgate|Kimberly Clark|General Mills|Kellogg|Post|Quaker|Campbell|Kraft Heinz|Mondelez|Mars|Hershey|Ferrero|Lindt|Godiva|Ghirardelli|Russell Stover|Fannie May) (Inc|LLC|Corp|Ltd|Co|Corporation|Company|Group|Holdings|Industries|Enterprises|Partners|Associates|Solutions|Technologies|Systems|Services|International|Global|Worldwide|Americas|USA|Digital|Interactive|Media|Entertainment|Financial|Healthcare|Pharmaceuticals|Biotech|Energy|Automotive|Aerospace|Defense|Retail|Hospitality|Logistics|Transportation|Communications|Telecommunications|Software|Hardware|Consulting|Advisory|Ventures|Capital|Investments|Properties|Realty|Development|Construction|Manufacturing|Engineering|Research|Analytics|Innovations|Labs|Studios|Productions|Networks|Platforms|Cloud|Mobile|Security|Data|AI|Robotics|Quantum|Nano|Micro|Macro|Mega|Ultra|Super|Hyper|Meta|Alpha|Beta|Gamma|Delta|Epsilon|Zeta|Eta|Theta|Iota|Kappa|Lambda|Mu|Nu|Xi|Omicron|Pi|Rho|Sigma|Tau|Upsilon|Phi|Chi|Psi|Omega)$"
            }
            Format::JobTitle => r"^.+$",
            Format::StreetAddress => r"^\d+ .+$",
            Format::City => {
                r"^(New York|Los Angeles|Chicago|Houston|Phoenix|Philadelphia|San Antonio|San Diego|Dallas|San Jose|Austin|Jacksonville|Fort Worth|Columbus|Charlotte|San Francisco|Indianapolis|Seattle|Denver|Washington|Boston|El Paso|Nashville|Detroit|Oklahoma City|Portland|Las Vegas|Memphis|Louisville|Baltimore|Milwaukee|Albuquerque|Tucson|Fresno|Mesa|Sacramento|Atlanta|Kansas City|Colorado Springs|Omaha|Raleigh|Miami|Long Beach|Virginia Beach|Oakland)$"
            }
            Format::State => {
                r"^(AL|AK|AZ|AR|CA|CO|CT|DE|FL|GA|HI|ID|IL|IN|IA|KS|KY|LA|ME|MD|MA|MI|MN|MS|MO|MT|NE|NV|NH|NJ|NM|NY|NC|ND|OH|OK|OR|PA|RI|SC|SD|TN|TX|UT|VT|VA|WA|WV|WI|WY)$"
            }
            Format::PostalCode => r"^\d{5}$",
            Format::Country => {
                r"^(United States|Canada|Mexico|United Kingdom|Germany|France|Italy|Spain|Australia|Brazil|Argentina|Japan|China|India|South Korea|Netherlands|Belgium|Switzerland|Sweden|Norway|Denmark|Finland|Poland|Austria|Greece|Portugal|Czech Republic|Hungary|Romania|Bulgaria|Croatia|Ireland|New Zealand|Singapore|Malaysia|Thailand|Indonesia|Philippines|Vietnam|Egypt|South Africa|Nigeria|Kenya|Morocco|Chile|Colombia|Peru|Venezuela|Ecuador|Uruguay)$"
            }
            Format::LoremIpsum(_) => r"^[a-z\s]+$",
            Format::ProductName => {
                r"^(Premium|Deluxe|Pro|Ultra|Super|Advanced|Professional) (Widget|Gadget|Device|Tool|System|Platform|Solution)$"
            }
            Format::ProductSku => r"^[A-Z]{3}-\d{4}$",
            Format::Version => r"^\d+\.\d+\.\d+$",
            Format::Hash => r"^[0-9a-f]{64}$",
            Format::UserAgent => r"^Mozilla/5\.0 .+$",
            Format::Email => r"^[a-z]{8}@(gmail\.com|yahoo\.com|outlook\.com|company\.com)$",
            Format::FirstName => {
                r"^(James|Mary|John|Patricia|Robert|Jennifer|Michael|Linda|William|Elizabeth|David|Barbara|Richard|Susan|Joseph|Jessica|Thomas|Sarah|Charles|Karen|Christopher|Nancy|Daniel|Lisa|Matthew|Betty|Anthony|Dorothy|Mark|Sandra|Donald|Ashley|Steven|Kimberly|Kenneth|Emily|Joshua|Michelle|Kevin|Carol|Brian|Amanda|George|Melissa|Edward|Deborah|Ronald|Stephanie|Timothy|Rebecca|Jason|Sharon|Jeffrey|Laura|Ryan|Cynthia|Jacob|Amy|Gary|Kathleen|Nicholas|Angela|Eric|Helen|Jonathan|Anna|Stephen|Brenda|Larry|Pamela|Justin|Nicole|Scott|Emma|Brandon|Samantha|Benjamin|Katherine|Samuel|Christine|Gregory|Catherine|Frank|Debra|Alexander|Rachel|Raymond|Carolyn|Patrick|Janet|Jack|Virginia|Dennis|Maria|Jerry|Heather|Tyler|Diane|Aaron|Ruth|Jose|Julie|Nathan|Olivia|Adam|Joyce|Harold|Victoria|Peter|Kelly|Henry|Christina|Zachary|Lauren|Douglas|Joan|Carl|Evelyn|Arthur|Judith|Albert|Megan|Willie|Cheryl|Austin|Martha|Jesse|Andrea|Gerald|Frances|Roger|Hannah|Keith|Jacqueline|Jeremy|Ann|Terry|Gloria|Lawrence|Jean|Sean|Kathryn|Christian|Alice|Ethan|Teresa|Bryan|Sara|Joe|Janice|Louis|Doris|Eugene|Madison|Russell|Julia|Gabriel|Grace|Bruce|Judy|Logan|Beverly|Juan|Denise|Elijah|Marilyn|Harry|Charlotte|Aaron|Marie|Willie|Abigail|Albert|Sophia|Jordan|Mia|Ralph|Isabella|Roy|Amber|Noah|Danielle|Mason|Brittany|Kyle|Rose|Francis|Diana|Russell|Natalie|Philip|Lori|Randy|Kayla|Vincent|Alexis|Billy|Lilly)$"
            }
            Format::LastName => {
                r"^(Smith|Johnson|Williams|Brown|Jones|Garcia|Miller|Davis|Rodriguez|Martinez|Hernandez|Lopez|Gonzalez|Wilson|Anderson|Thomas|Taylor|Moore|Jackson|Martin|Lee|Perez|Thompson|White|Harris|Sanchez|Clark|Ramirez|Lewis|Robinson|Walker|Young|Allen|King|Wright|Scott|Torres|Nguyen|Hill|Flores|Green|Adams|Nelson|Baker|Hall|Rivera|Campbell|Mitchell|Carter|Roberts|Gomez|Phillips|Evans|Turner|Diaz|Parker|Cruz|Edwards|Collins|Reyes|Stewart|Morris|Morales|Murphy|Cook|Rogers|Gutierrez|Ortiz|Morgan|Cooper|Peterson|Bailey|Reed|Kelly|Howard|Ramos|Kim|Cox|Ward|Richardson|Watson|Brooks|Chavez|Wood|James|Bennett|Gray|Mendoza|Ruiz|Hughes|Price|Alvarez|Castillo|Sanders|Patel|Myers|Long|Ross|Foster|Jimenez|Powell|Jenkins|Perry|Russell|Sullivan|Bell|Coleman|Butler|Henderson|Barnes|Gonzales|Fisher|Vasquez|Simmons|Romero|Jordan|Patterson|Alexander|Hamilton|Graham|Reynolds|Griffin|Wallace|Moreno|West|Cole|Hayes|Bryant|Herrera|Gibson|Ellis|Tran|Medina|Aguilar|Stevens|Murray|Ford|Castro|Marshall|Owens|Harrison|Fernandez|Mcdonald|Woods|Washington|Kennedy|Wells|Vargas|Henry|Chen|Freeman|Webb|Tucker|Guzman|Burns|Crawford|Olson|Simpson|Porter|Hunter|Gordon|Mendez|Silva|Shaw|Snyder|Mason|Dixon|Munoz|Hunt|Hicks|Holmes|Palmer|Wagner|Black|Robertson|Boyd|Rose|Stone|Salazar|Fox|Warren|Mills|Meyer|Rice|Schmidt|Garza|Daniels|Ferguson|Nichols|Stephens|Soto|Weaver|Ryan|Gardner|Payne|Grant|Dunn|Kelley|Spencer|Hawkins|Arnold|Pierce|Vazquez|Hansen|Peters|Santos|Hart|Bradley|Knight|Elliott|Cunningham|Duncan|Armstrong|Hudson|Carroll|Lane|Riley|Andrews|Alvarado|Ray|Delgado|Berry|Perkins|Hoffman|Johnston|Matthews|Pena|Richards|Contreras|Willis|Carpenter|Lawrence|Sandoval|Guerrero|George|Chapman|Rios|Estrada|Ortega|Watkins|Greene|Nunez|Wheeler|Valdez|Harper|Burke|Larson|Santiago|Maldonado|Morrison|Franklin|Carlson|Austin|Dominguez|Carr|Lawson|Jacobs|Obrien|Lynch|Singh|Vega|Bishop|Montgomery|Oliver|Jensen|Harvey|Williamson|Gilbert|Dean|Sims|Espinoza|Howell|Li|Wong|Reid|Hanson|Le|Mccoy|Garrett|Burton|Fuller|Wang|Weber|Welch|Rojas|Lucas|Marquez|Fields|Park|Yang|Little|Banks|Padilla|Day|Walsh|Bowman|Schultz|Luna|Fowler|Mejia)$"
            }
            Format::FullName => {
                r"^(James|Mary|John|Patricia|Robert|Jennifer|Michael|Linda|William|Elizabeth|David|Barbara|Richard|Susan|Joseph|Jessica|Thomas|Sarah|Charles|Karen|Christopher|Nancy|Daniel|Lisa|Matthew|Betty|Anthony|Dorothy|Mark|Sandra|Donald|Ashley|Steven|Kimberly|Kenneth|Emily|Joshua|Michelle|Kevin|Carol|Brian|Amanda|George|Melissa|Edward|Deborah|Ronald|Stephanie|Timothy|Rebecca|Jason|Sharon|Jeffrey|Laura|Ryan|Cynthia|Jacob|Amy|Gary|Kathleen|Nicholas|Angela|Eric|Helen|Jonathan|Anna|Stephen|Brenda|Larry|Pamela|Justin|Nicole|Scott|Emma|Brandon|Samantha|Benjamin|Katherine|Samuel|Christine|Gregory|Catherine|Frank|Debra|Alexander|Rachel|Raymond|Carolyn|Patrick|Janet|Jack|Virginia|Dennis|Maria|Jerry|Heather|Tyler|Diane|Aaron|Ruth|Jose|Julie|Nathan|Olivia|Adam|Joyce|Harold|Victoria|Peter|Kelly|Henry|Christina|Zachary|Lauren|Douglas|Joan|Carl|Evelyn|Arthur|Judith|Albert|Megan|Willie|Cheryl|Austin|Martha|Jesse|Andrea|Gerald|Frances|Roger|Hannah|Keith|Jacqueline|Jeremy|Ann|Terry|Gloria|Lawrence|Jean|Sean|Kathryn|Christian|Alice|Ethan|Teresa|Bryan|Sara|Joe|Janice|Louis|Doris|Eugene|Madison|Russell|Julia|Gabriel|Grace|Bruce|Judy|Logan|Beverly|Juan|Denise|Elijah|Marilyn|Harry|Charlotte|Aaron|Marie|Willie|Abigail|Albert|Sophia|Jordan|Mia|Ralph|Isabella|Roy|Amber|Noah|Danielle|Mason|Brittany|Kyle|Rose|Francis|Diana|Russell|Natalie|Philip|Lori|Randy|Kayla|Vincent|Alexis|Billy|Lilly) (Smith|Johnson|Williams|Brown|Jones|Garcia|Miller|Davis|Rodriguez|Martinez|Hernandez|Lopez|Gonzalez|Wilson|Anderson|Thomas|Taylor|Moore|Jackson|Martin|Lee|Perez|Thompson|White|Harris|Sanchez|Clark|Ramirez|Lewis|Robinson|Walker|Young|Allen|King|Wright|Scott|Torres|Nguyen|Hill|Flores|Green|Adams|Nelson|Baker|Hall|Rivera|Campbell|Mitchell|Carter|Roberts|Gomez|Phillips|Evans|Turner|Diaz|Parker|Cruz|Edwards|Collins|Reyes|Stewart|Morris|Morales|Murphy|Cook|Rogers|Gutierrez|Ortiz|Morgan|Cooper|Peterson|Bailey|Reed|Kelly|Howard|Ramos|Kim|Cox|Ward|Richardson|Watson|Brooks|Chavez|Wood|James|Bennett|Gray|Mendoza|Ruiz|Hughes|Price|Alvarez|Castillo|Sanders|Patel|Myers|Long|Ross|Foster|Jimenez|Powell|Jenkins|Perry|Russell|Sullivan|Bell|Coleman|Butler|Henderson|Barnes|Gonzales|Fisher|Vasquez|Simmons|Romero|Jordan|Patterson|Alexander|Hamilton|Graham|Reynolds|Griffin|Wallace|Moreno|West|Cole|Hayes|Bryant|Herrera|Gibson|Ellis|Tran|Medina|Aguilar|Stevens|Murray|Ford|Castro|Marshall|Owens|Harrison|Fernandez|Mcdonald|Woods|Washington|Kennedy|Wells|Vargas|Henry|Chen|Freeman|Webb|Tucker|Guzman|Burns|Crawford|Olson|Simpson|Porter|Hunter|Gordon|Mendez|Silva|Shaw|Snyder|Mason|Dixon|Munoz|Hunt|Hicks|Holmes|Palmer|Wagner|Black|Robertson|Boyd|Rose|Stone|Salazar|Fox|Warren|Mills|Meyer|Rice|Schmidt|Garza|Daniels|Ferguson|Nichols|Stephens|Soto|Weaver|Ryan|Gardner|Payne|Grant|Dunn|Kelley|Spencer|Hawkins|Arnold|Pierce|Vazquez|Hansen|Peters|Santos|Hart|Bradley|Knight|Elliott|Cunningham|Duncan|Armstrong|Hudson|Carroll|Lane|Riley|Andrews|Alvarado|Ray|Delgado|Berry|Perkins|Hoffman|Johnston|Matthews|Pena|Richards|Contreras|Willis|Carpenter|Lawrence|Sandoval|Guerrero|George|Chapman|Rios|Estrada|Ortega|Watkins|Greene|Nunez|Wheeler|Valdez|Harper|Burke|Larson|Santiago|Maldonado|Morrison|Franklin|Carlson|Austin|Dominguez|Carr|Lawson|Jacobs|Obrien|Lynch|Singh|Vega|Bishop|Montgomery|Oliver|Jensen|Harvey|Williamson|Gilbert|Dean|Sims|Espinoza|Howell|Li|Wong|Reid|Hanson|Le|Mccoy|Garrett|Burton|Fuller|Wang|Weber|Welch|Rojas|Lucas|Marquez|Fields|Park|Yang|Little|Banks|Padilla|Day|Walsh|Bowman|Schultz|Luna|Fowler|Mejia)$"
            }
            Format::PhoneNumber => r"^(\+\d{1,2}\s)?(\(\d{3}\)\s?|\d{3})[\.\-]?\d{3}[\.\-]?\d{4}$",
            Format::Iso8601DurationString => {
                r"^P((\d{1,2}Y)?(0?[0-9]|1[0-1]M)?(\d{1,4}W)?([0-2]?[0-9]D)?)?(T([0-1]?[0-9]|2[0-3]H)?([0-5]?[0-9]M)?([0-5]?[0-9](\.\d{1,3})?S)?)?$"
            }
            Format::TimeZone => {
                r"^(Africa/Abidjan|Africa/Accra|Africa/Addis_Ababa|Africa/Algiers|Africa/Asmara|Africa/Bamako|Africa/Bangui|Africa/Banjul|Africa/Bissau|Africa/Blantyre|Africa/Brazzaville|Africa/Bujumbura|Africa/Cairo|Africa/Casablanca|Africa/Ceuta|Africa/Conakry|Africa/Dakar|Africa/Dar_es_Salaam|Africa/Djibouti|Africa/Douala|Africa/El_Aaiun|Africa/Freetown|Africa/Gaborone|Africa/Harare|Africa/Johannesburg|Africa/Juba|Africa/Kampala|Africa/Khartoum|Africa/Kigali|Africa/Kinshasa|Africa/Lagos|Africa/Libreville|Africa/Lome|Africa/Luanda|Africa/Lubumbashi|Africa/Lusaka|Africa/Malabo|Africa/Maputo|Africa/Maseru|Africa/Mbabane|Africa/Mogadishu|Africa/Monrovia|Africa/Nairobi|Africa/Ndjamena|Africa/Niamey|Africa/Nouakchott|Africa/Ouagadougou|Africa/Porto-Novo|Africa/Sao_Tome|Africa/Tripoli|Africa/Tunis|Africa/Windhoek|America/Adak|America/Anchorage|America/Anguilla|America/Antigua|America/Araguaina|America/Argentina/Buenos_Aires|America/Argentina/Catamarca|America/Argentina/Cordoba|America/Argentina/Jujuy|America/Argentina/La_Rioja|America/Argentina/Mendoza|America/Argentina/Rio_Gallegos|America/Argentina/Salta|America/Argentina/San_Juan|America/Argentina/San_Luis|America/Argentina/Tucuman|America/Argentina/Ushuaia|America/Aruba|America/Asuncion|America/Atikokan|America/Bahia|America/Bahia_Banderas|America/Barbados|America/Belem|America/Belize|America/Blanc-Sablon|America/Boa_Vista|America/Bogota|America/Boise|America/Cambridge_Bay|America/Campo_Grande|America/Cancun|America/Caracas|America/Cayenne|America/Cayman|America/Chicago|America/Chihuahua|America/Costa_Rica|America/Creston|America/Cuiaba|America/Curacao|America/Danmarkshavn|America/Dawson|America/Dawson_Creek|America/Denver|America/Detroit|America/Dominica|America/Edmonton|America/Eirunepe|America/El_Salvador|America/Fort_Nelson|America/Fortaleza|America/Glace_Bay|America/Goose_Bay|America/Grand_Turk|America/Grenada|America/Guadeloupe|America/Guatemala|America/Guayaquil|America/Guyana|America/Halifax|America/Havana|America/Hermosillo|America/Indiana/Indianapolis|America/Indiana/Knox|America/Indiana/Marengo|America/Indiana/Petersburg|America/Indiana/Tell_City|America/Indiana/Vevay|America/Indiana/Vincennes|America/Indiana/Winamac|America/Inuvik|America/Iqaluit|America/Jamaica|America/Juneau|America/Kentucky/Louisville|America/Kentucky/Monticello|America/Kralendijk|America/La_Paz|America/Lima|America/Los_Angeles|America/Lower_Princes|America/Maceio|America/Managua|America/Manaus|America/Marigot|America/Martinique|America/Matamoros|America/Mazatlan|America/Menominee|America/Merida|America/Metlakatla|America/Mexico_City|America/Miquelon|America/Moncton|America/Monterrey|America/Montevideo|America/Montserrat|America/Nassau|America/New_York|America/Nipigon|America/Nome|America/Noronha|America/North_Dakota/Beulah|America/North_Dakota/Center|America/North_Dakota/New_Salem|America/Nuuk|America/Ojinaga|America/Panama|America/Pangnirtung|America/Paramaribo|America/Phoenix|America/Port-au-Prince|America/Port_of_Spain|America/Porto_Velho|America/Puerto_Rico|America/Punta_Arenas|America/Rainy_River|America/Rankin_Inlet|America/Recife|America/Regina|America/Resolute|America/Rio_Branco|America/Santarem|America/Santiago|America/Santo_Domingo|America/Sao_Paulo|America/Scoresbysund|America/Sitka|America/St_Barthelemy|America/St_Johns|America/St_Kitts|America/St_Lucia|America/St_Thomas|America/St_Vincent|America/Swift_Current|America/Tegucigalpa|America/Thule|America/Thunder_Bay|America/Tijuana|America/Toronto|America/Tortola|America/Vancouver|America/Whitehorse|America/Winnipeg|America/Yakutat|America/Yellowknife|Antarctica/Casey|Antarctica/Davis|Antarctica/DumontDUrville|Antarctica/Macquarie|Antarctica/Mawson|Antarctica/McMurdo|Antarctica/Palmer|Antarctica/Rothera|Antarctica/Syowa|Antarctica/Troll|Antarctica/Vostok|Arctic/Longyearbyen|Asia/Aden|Asia/Almaty|Asia/Amman|Asia/Anadyr|Asia/Aqtau|Asia/Aqtobe|Asia/Ashgabat|Asia/Atyrau|Asia/Baghdad|Asia/Bahrain|Asia/Baku|Asia/Bangkok|Asia/Barnaul|Asia/Beirut|Asia/Bishkek|Asia/Brunei|Asia/Chita|Asia/Choibalsan|Asia/Colombo|Asia/Damascus|Asia/Dhaka|Asia/Dili|Asia/Dubai|Asia/Dushanbe|Asia/Famagusta|Asia/Gaza|Asia/Hebron|Asia/Ho_Chi_Minh|Asia/Hong_Kong|Asia/Hovd|Asia/Irkutsk|Asia/Jakarta|Asia/Jayapura|Asia/Jerusalem|Asia/Kabul|Asia/Kamchatka|Asia/Karachi|Asia/Kathmandu|Asia/Khandyga|Asia/Kolkata|Asia/Krasnoyarsk|Asia/Kuala_Lumpur|Asia/Kuching|Asia/Kuwait|Asia/Macau|Asia/Magadan|Asia/Makassar|Asia/Manila|Asia/Muscat|Asia/Nicosia|Asia/Novokuznetsk|Asia/Novosibirsk|Asia/Omsk|Asia/Oral|Asia/Phnom_Penh|Asia/Pontianak|Asia/Pyongyang|Asia/Qatar|Asia/Qostanay|Asia/Qyzylorda|Asia/Riyadh|Asia/Sakhalin|Asia/Samarkand|Asia/Seoul|Asia/Shanghai|Asia/Singapore|Asia/Srednekolymsk|Asia/Taipei|Asia/Tashkent|Asia/Tbilisi|Asia/Tehran|Asia/Thimphu|Asia/Tokyo|Asia/Tomsk|Asia/Ulaanbaatar|Asia/Urumqi|Asia/Ust-Nera|Asia/Vientiane|Asia/Vladivostok|Asia/Yakutsk|Asia/Yangon|Asia/Yekaterinburg|Asia/Yerevan|Atlantic/Azores|Atlantic/Bermuda|Atlantic/Canary|Atlantic/Cape_Verde|Atlantic/Faroe|Atlantic/Madeira|Atlantic/Reykjavik|Atlantic/South_Georgia|Atlantic/St_Helena|Atlantic/Stanley|Australia/Adelaide|Australia/Brisbane|Australia/Broken_Hill|Australia/Darwin|Australia/Eucla|Australia/Hobart|Australia/Lindeman|Australia/Lord_Howe|Australia/Melbourne|Australia/Perth|Australia/Sydney|Europe/Amsterdam|Europe/Andorra|Europe/Astrakhan|Europe/Athens|Europe/Belgrade|Europe/Berlin|Europe/Bratislava|Europe/Brussels|Europe/Bucharest|Europe/Budapest|Europe/Busingen|Europe/Chisinau|Europe/Copenhagen|Europe/Dublin|Europe/Gibraltar|Europe/Guernsey|Europe/Helsinki|Europe/Isle_of_Man|Europe/Istanbul|Europe/Jersey|Europe/Kaliningrad|Europe/Kiev|Europe/Kirov|Europe/Lisbon|Europe/Ljubljana|Europe/London|Europe/Luxembourg|Europe/Madrid|Europe/Malta|Europe/Mariehamn|Europe/Minsk|Europe/Monaco|Europe/Moscow|Europe/Oslo|Europe/Paris|Europe/Podgorica|Europe/Prague|Europe/Riga|Europe/Rome|Europe/Samara|Europe/San_Marino|Europe/Sarajevo|Europe/Saratov|Europe/Simferopol|Europe/Skopje|Europe/Sofia|Europe/Stockholm|Europe/Tallinn|Europe/Tirane|Europe/Ulyanovsk|Europe/Uzhgorod|Europe/Vaduz|Europe/Vatican|Europe/Vienna|Europe/Vilnius|Europe/Volgograd|Europe/Warsaw|Europe/Zagreb|Europe/Zaporozhye|Europe/Zurich|Indian/Antananarivo|Indian/Chagos|Indian/Christmas|Indian/Cocos|Indian/Comoro|Indian/Kerguelen|Indian/Mahe|Indian/Maldives|Indian/Mauritius|Indian/Mayotte|Indian/Reunion|Pacific/Apia|Pacific/Auckland|Pacific/Bougainville|Pacific/Chatham|Pacific/Chuuk|Pacific/Easter|Pacific/Efate|Pacific/Enderbury|Pacific/Fakaofo|Pacific/Fiji|Pacific/Funafuti|Pacific/Galapagos|Pacific/Gambier|Pacific/Guadalcanal|Pacific/Guam|Pacific/Honolulu|Pacific/Kanton|Pacific/Kiritimati|Pacific/Kosrae|Pacific/Kwajalein|Pacific/Majuro|Pacific/Marquesas|Pacific/Midway|Pacific/Nauru|Pacific/Niue|Pacific/Norfolk|Pacific/Noumea|Pacific/Pago_Pago|Pacific/Palau|Pacific/Pitcairn|Pacific/Pohnpei|Pacific/Port_Moresby|Pacific/Rarotonga|Pacific/Saipan|Pacific/Tahiti|Pacific/Tarawa|Pacific/Tongatapu|Pacific/Wake|Pacific/Wallis)$"
            }
            Format::DateWithinDays(days) => {
                // Use the helper function to generate date range pattern
                let date_pattern = generate_date_range_pattern(days);

                &format!(r"^{}T([01][0-9]|2[0-3]):(00|15|30|45):00Z$", date_pattern)
            }
            Format::AppointmentDateTime => {
                // Use the helper function to generate date range pattern for next 10 days
                let date_pattern = generate_date_range_pattern(10);

                // Create a regex pattern for appointment hours (7am-8pm)
                &format!(r"^{}T(0[7-9]|1[0-9]|20):(00|15|30|45):00Z$", date_pattern)
            }
            Format::TailwindColorSet(_) => {
                r#"^\{\"main\":\"\#[0-9a-fA-F]{6}\",\"hover\":\"\#[0-9a-fA-F]{6}\",\"active\":\"\#[0-9a-fA-F]{6}\"\}$"#
            }
            Format::Custom(pattern) => &pattern.clone(),

            Format::Random => r"^[a-zA-Z0-9]{8,16}$",

            Format::AppointmentDurationNs => {
                // 1-5 hours in nanoseconds, in 15-minute increments
                // 15 minutes = 900,000,000,000 ns
                // 1 hour = 3,600,000,000,000 ns
                // 5 hours = 18,000,000,000,000 ns
                // Exhaustive list of all valid values wrapped in duration::from::nanos():
                r"^(duration::from::nanos\(3600000000000\)|duration::from::nanos\(4500000000000\)|duration::from::nanos\(5400000000000\)|duration::from::nanos\(6300000000000\)|duration::from::nanos\(7200000000000\)|duration::from::nanos\(8100000000000\)|duration::from::nanos\(9000000000000\)|duration::from::nanos\(9900000000000\)|duration::from::nanos\(10800000000000\)|duration::from::nanos\(11700000000000\)|duration::from::nanos\(12600000000000\)|duration::from::nanos\(13500000000000\)|duration::from::nanos\(14400000000000\)|duration::from::nanos\(15300000000000\)|duration::from::nanos\(16200000000000\)|duration::from::nanos\(17100000000000\)|duration::from::nanos\(18000000000000\))$"
            }
        };

        let regex = Regex::new(pattern).expect("Failed to create regex from Format");
        tracing::trace!(pattern_length = pattern.len(), "Regex created successfully");
        regex
    }
}

impl ToTokens for Format {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let variant_tokens = match self {
            Format::Uuid => quote! { ::evenframe::schemasync::format::Format::Uuid },
            Format::DateTime => {
                quote! { ::evenframe::schemasync::format::Format::DateTime }
            }
            Format::Date => quote! { ::evenframe::schemasync::format::Format::Date },
            Format::Time => quote! { ::evenframe::schemasync::format::Format::Time },
            Format::HexString(len) => {
                quote! { ::evenframe::schemasync::format::Format::HexString(#len) }
            }
            Format::Base64String(len) => {
                quote! { ::evenframe::schemasync::format::Format::Base64String(#len) }
            }
            Format::JwtToken => {
                quote! { ::evenframe::schemasync::format::Format::JwtToken }
            }
            Format::CreditCardNumber => {
                quote! { ::evenframe::schemasync::format::Format::CreditCardNumber }
            }
            Format::SocialSecurityNumber => {
                quote! { ::evenframe::schemasync::format::Format::SocialSecurityNumber }
            }
            Format::IpAddress => {
                quote! { ::evenframe::schemasync::format::Format::IpAddress }
            }
            Format::MacAddress => {
                quote! { ::evenframe::schemasync::format::Format::MacAddress }
            }
            Format::ColorHex => {
                quote! { ::evenframe::schemasync::format::Format::ColorHex }
            }
            Format::Oklch => quote! { ::evenframe::schemasync::format::Format::Oklch },
            Format::Filename(ext) => {
                quote! { ::evenframe::schemasync::format::Format::Filename(#ext.to_string()) }
            }
            Format::Url(domain) => {
                quote! { ::evenframe::schemasync::format::Format::Url(#domain.to_string()) }
            }
            Format::CurrencyAmount => {
                quote! { ::evenframe::schemasync::format::Format::CurrencyAmount }
            }
            Format::Percentage => {
                quote! { ::evenframe::schemasync::format::Format::Percentage }
            }
            Format::Latitude => {
                quote! { ::evenframe::schemasync::format::Format::Latitude }
            }
            Format::Longitude => {
                quote! { ::evenframe::schemasync::format::Format::Longitude }
            }
            Format::CompanyName => {
                quote! { ::evenframe::schemasync::format::Format::CompanyName }
            }
            Format::JobTitle => {
                quote! { ::evenframe::schemasync::format::Format::JobTitle }
            }
            Format::StreetAddress => {
                quote! { ::evenframe::schemasync::format::Format::StreetAddress }
            }
            Format::City => quote! { ::evenframe::schemasync::format::Format::City },
            Format::State => quote! { ::evenframe::schemasync::format::Format::State },
            Format::PostalCode => {
                quote! { ::evenframe::schemasync::format::Format::PostalCode }
            }
            Format::Country => quote! { ::evenframe::schemasync::format::Format::Country },
            Format::LoremIpsum(words) => {
                quote! { ::evenframe::schemasync::format::Format::LoremIpsum(#words) }
            }
            Format::ProductName => {
                quote! { ::evenframe::schemasync::format::Format::ProductName }
            }
            Format::ProductSku => {
                quote! { ::evenframe::schemasync::format::Format::ProductSku }
            }
            Format::Version => quote! { ::evenframe::schemasync::format::Format::Version },
            Format::Hash => quote! { ::evenframe::schemasync::format::Format::Hash },
            Format::UserAgent => {
                quote! { ::evenframe::schemasync::format::Format::UserAgent }
            }
            Format::Email => quote! { ::evenframe::schemasync::format::Format::Email },
            Format::FirstName => {
                quote! { ::evenframe::schemasync::format::Format::FirstName }
            }
            Format::LastName => {
                quote! { ::evenframe::schemasync::format::Format::LastName }
            }
            Format::FullName => {
                quote! { ::evenframe::schemasync::format::Format::FullName }
            }
            Format::PhoneNumber => {
                quote! { ::evenframe::schemasync::format::Format::PhoneNumber }
            }
            Format::Iso8601DurationString => {
                quote! { ::evenframe::schemasync::format::Format::Iso8601DurationString }
            }
            Format::TimeZone => {
                quote! { ::evenframe::schemasync::format::Format::TimeZone }
            }
            Format::DateWithinDays(days) => {
                quote! { ::evenframe::schemasync::format::Format::DateWithinDays(#days) }
            }
            Format::AppointmentDateTime => {
                quote! { ::evenframe::schemasync::format::Format::AppointmentDateTime }
            }
            Format::TailwindColorSet(color) => match color {
                Some(c) => {
                    quote! { ::evenframe::schemasync::format::Format::TailwindColorSet(Some(#c.to_string())) }
                }
                None => {
                    quote! { ::evenframe::schemasync::format::Format::TailwindColorSet(None) }
                }
            },
            Format::Custom(pattern) => {
                quote! { ::evenframe::schemasync::format::Format::Custom(#pattern.to_string()) }
            }
            Format::Random => {
                quote! { ::evenframe::schemasync::format::Format::Random }
            }
            Format::AppointmentDurationNs => {
                quote! { ::evenframe::schemasync::format::Format::AppointmentDurationNs }
            }
        };

        tokens.extend(variant_tokens);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_format() {
        let format = Format::Uuid;
        let value = format.generate_formatted_value();
        let regex = format.into_regex();
        assert!(
            regex.is_match(&value),
            "Generated UUID {} doesn't match pattern",
            value
        );
    }

    #[test]
    fn test_datetime_format() {
        let format = Format::DateTime;
        let value = format.generate_formatted_value();
        let regex = format.into_regex();
        assert!(
            regex.is_match(&value),
            "Generated DateTime {} doesn't match pattern",
            value
        );
    }

    #[test]
    fn test_hex_string_format() {
        let format = Format::HexString(8);
        let value = format.generate_formatted_value();
        assert_eq!(value.len(), 8);
        assert!(value.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_email_format() {
        let format = Format::Email;
        let value = format.generate_formatted_value();
        let regex = format.into_regex();
        assert!(
            regex.is_match(&value),
            "Generated Email {} doesn't match pattern",
            value
        );
    }

    #[test]
    fn test_phone_number_format() {
        let format = Format::PhoneNumber;
        let value = format.generate_formatted_value();
        println!("Generated phone number: {}", value);

        // Phone number should match the pattern
        let regex = format.into_regex();
        assert!(
            regex.is_match(&value),
            "Generated phone number {} doesn't match pattern",
            value
        );

        // Generate multiple samples to see variety
        println!("Multiple phone number samples:");
        for _ in 0..10 {
            let phone = Format::PhoneNumber.generate_formatted_value();
            println!("  {}", phone);
        }
    }

    #[test]
    fn test_ip_address_format() {
        let format = Format::IpAddress;
        let value = format.generate_formatted_value();
        let regex = format.into_regex();
        assert!(
            regex.is_match(&value),
            "Generated IP {} doesn't match pattern",
            value
        );
    }

    #[test]
    fn test_various_formats() {
        let formats = vec![
            Format::Date,
            Format::Time,
            Format::ColorHex,
            Format::MacAddress,
            Format::PostalCode,
            Format::Version,
            Format::ProductSku,
        ];

        for format in formats {
            let value = format.generate_formatted_value();
            let regex = format.clone().into_regex();
            assert!(
                regex.is_match(&value),
                "Generated value '{}' for {:?} doesn't match pattern",
                value,
                format
            );
        }
    }

    #[test]
    fn test_name_formats() {
        // Test FirstName format generates real first names
        let first_name_format = Format::FirstName;
        let first_name = first_name_format.generate_formatted_value();
        println!("Generated First Name: {}", first_name);
        assert!(!first_name.is_empty(), "First name should not be empty");
        assert!(
            first_name.chars().all(|c| c.is_alphabetic()),
            "First name should only contain letters"
        );

        // Test LastName format generates real last names
        let last_name_format = Format::LastName;
        let last_name = last_name_format.generate_formatted_value();
        println!("Generated Last Name: {}", last_name);
        assert!(!last_name.is_empty(), "Last name should not be empty");
        assert!(
            last_name.chars().all(|c| c.is_alphabetic()),
            "Last name should only contain letters"
        );

        // Test FullName format generates real full names
        let full_name_format = Format::FullName;
        let full_name = full_name_format.generate_formatted_value();
        println!("Generated Full Name: {}", full_name);
        let parts: Vec<&str> = full_name.split_whitespace().collect();
        assert_eq!(parts.len(), 2, "Full name should have exactly 2 parts");

        // Generate multiple samples to see variety
        println!("\nGenerating multiple name samples:");
        for _ in 0..10 {
            println!(
                "  {} {} ({})",
                Format::FirstName.generate_formatted_value(),
                Format::LastName.generate_formatted_value(),
                Format::FullName.generate_formatted_value()
            );
        }

        // Verify the pattern matching works correctly
        let first_regex = Format::FirstName.into_regex();
        let last_regex = Format::LastName.into_regex();
        let full_regex = Format::FullName.into_regex();

        for _ in 0..10 {
            let first = Format::FirstName.generate_formatted_value();
            let last = Format::LastName.generate_formatted_value();
            let full = Format::FullName.generate_formatted_value();

            assert!(
                first_regex.is_match(&first),
                "First name '{}' doesn't match regex",
                first
            );
            assert!(
                last_regex.is_match(&last),
                "Last name '{}' doesn't match regex",
                last
            );
            assert!(
                full_regex.is_match(&full),
                "Full name '{}' doesn't match regex",
                full
            );
        }
    }

    #[test]
    fn test_timezone_format() {
        let timezone_format = Format::TimeZone;
        let timezone = timezone_format.generate_formatted_value();
        println!("Generated TimeZone: {}", timezone);

        // Verify it has the correct format (Continent/City or special cases)
        assert!(
            timezone.contains('/') || timezone.starts_with("UTC"),
            "Timezone should contain '/' or be UTC"
        );

        // Generate multiple samples to see variety
        println!("\nGenerating multiple timezone samples:");
        for _ in 0..10 {
            let tz = Format::TimeZone.generate_formatted_value();
            println!("  {}", tz);
        }

        // Verify pattern matching
        let tz_regex = Format::TimeZone.into_regex();
        for _ in 0..10 {
            let tz = Format::TimeZone.generate_formatted_value();
            assert!(
                tz_regex.is_match(&tz),
                "Timezone '{}' doesn't match regex",
                tz
            );
        }
    }

    #[test]
    fn test_duration_format() {
        let duration_format = Format::Iso8601DurationString;
        let duration = duration_format.generate_formatted_value();
        println!("Generated Iso8601DurationString: {}", duration);

        // Verify it starts with P
        assert!(
            duration.starts_with('P'),
            "Iso8601DurationString should start with 'P'"
        );

        // Verify it's a valid ISO 8601 duration
        assert!(
            duration.contains('Y')
                || duration.contains('M')
                || duration.contains('W')
                || duration.contains('D')
                || duration.contains('T')
                || duration.contains('H')
                || duration.contains('S'),
            "Iso8601DurationString should contain at least one time unit"
        );

        // Generate multiple samples to see variety
        println!("\nGenerating multiple duration samples:");
        for _ in 0..15 {
            let dur = Format::Iso8601DurationString.generate_formatted_value();
            println!("  {}", dur);
        }

        // Verify pattern matching
        let dur_regex = Format::Iso8601DurationString.into_regex();
        for _ in 0..20 {
            let dur = Format::Iso8601DurationString.generate_formatted_value();
            assert!(
                dur_regex.is_match(&dur),
                "Iso8601DurationString '{}' doesn't match regex",
                dur
            );
        }
    }

    #[test]
    fn test_city_state_country() {
        // Test City format generates real city names
        let city_format = Format::City;
        let city = city_format.generate_formatted_value();
        println!("Generated City: {}", city);
        // Just verify it's not empty and looks like a city name (contains letters and possibly spaces)
        assert!(!city.is_empty(), "City should not be empty");
        assert!(
            city.chars().any(|c| c.is_alphabetic()),
            "City should contain letters"
        );

        // Test State format generates real state codes
        let state_format = Format::State;
        let state = state_format.generate_formatted_value();
        println!("Generated State: {}", state);
        assert!(state.len() == 2, "State code should be 2 characters");
        assert!(
            state.chars().all(|c| c.is_ascii_uppercase()),
            "State code should be uppercase"
        );

        // Test Country format generates real country names
        let country_format = Format::Country;
        let country = country_format.generate_formatted_value();
        println!("Generated Country: {}", country);
        // Just verify it's not empty and looks like a country name
        assert!(!country.is_empty(), "Country should not be empty");
        assert!(
            country.chars().any(|c| c.is_alphabetic()),
            "Country should contain letters"
        );

        // Generate multiple samples to ensure variety
        println!("\nGenerating multiple samples:");
        for _ in 0..10 {
            println!(
                "  City: {}, State: {}, Country: {}",
                Format::City.generate_formatted_value(),
                Format::State.generate_formatted_value(),
                Format::Country.generate_formatted_value()
            );
        }

        // Verify the pattern matching works correctly
        let city_regex = Format::City.into_regex();
        let state_regex = Format::State.into_regex();
        let country_regex = Format::Country.into_regex();

        for _ in 0..10 {
            let city = Format::City.generate_formatted_value();
            let state = Format::State.generate_formatted_value();
            let country = Format::Country.generate_formatted_value();

            assert!(
                city_regex.is_match(&city),
                "City '{}' doesn't match regex",
                city
            );
            assert!(
                state_regex.is_match(&state),
                "State '{}' doesn't match regex",
                state
            );
            assert!(
                country_regex.is_match(&country),
                "Country '{}' doesn't match regex",
                country
            );
        }
    }
}
