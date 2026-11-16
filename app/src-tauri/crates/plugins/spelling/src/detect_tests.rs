use super::*;
use crate::OFFICIAL;

fn ids() -> Vec<&'static str> {
    OFFICIAL.iter().map(|l| l.id).collect()
}

/// Everyday sentences per language (the tag the detector should answer, Galician has no detector model).
const CORPUS: [(&str, [&str; 10]); 13] = [
    (
        "en",
        [
            "I forgot my umbrella at home again, so I got soaked on the way to work.",
            "The bakery on the corner sells the best bread in town, but it closes early.",
            "Could you call me back when you have a minute? It is about the meeting tomorrow.",
            "We spent the whole weekend painting the kitchen and it still looks unfinished.",
            "My brother moved to a small village last year and he says he will never go back to the city.",
            "Please remember to water the plants while I am away on holiday.",
            "The train was late this morning because of a problem with the signals.",
            "She has been learning to play the piano since she was seven years old.",
            "If it does not rain on Saturday, we could have a picnic by the river.",
            "I usually read a few pages of a book before I fall asleep at night.",
        ],
    ),
    (
        "pt-BR",
        [
            "Esqueci o guarda-chuva em casa de novo e cheguei todo molhado no trabalho.",
            "A padaria da esquina vende o melhor pão do bairro, mas fecha cedo.",
            "Você pode me ligar quando tiver um minuto? É sobre a reunião de amanhã.",
            "Passamos o fim de semana inteiro pintando a cozinha e ainda parece inacabada.",
            "Meu irmão se mudou para uma cidade pequena no ano passado e diz que nunca vai voltar.",
            "Não se esqueça de regar as plantas enquanto eu estiver viajando.",
            "O ônibus atrasou hoje de manhã por causa de um acidente na avenida.",
            "Ela está aprendendo a tocar piano desde que tinha sete anos.",
            "Se não chover no sábado, a gente podia fazer um piquenique perto do rio.",
            "Eu costumo ler algumas páginas de um livro antes de dormir.",
        ],
    ),
    (
        "es",
        [
            "Otra vez me olvidé el paraguas en casa y llegué empapado al trabajo.",
            "La panadería de la esquina vende el mejor pan del barrio, pero cierra temprano.",
            "¿Puedes llamarme cuando tengas un momento? Es sobre la reunión de mañana.",
            "Pasamos todo el fin de semana pintando la cocina y todavía parece sin terminar.",
            "Mi hermano se mudó a un pueblo pequeño el año pasado y dice que nunca volverá a la ciudad.",
            "Acuérdate de regar las plantas mientras estoy de vacaciones.",
            "El tren llegó tarde esta mañana por un problema con las señales.",
            "Ella aprende a tocar el piano desde que tenía siete años.",
            "Si no llueve el sábado, podríamos hacer un pícnic junto al río.",
            "Suelo leer unas cuantas páginas de un libro antes de dormirme.",
        ],
    ),
    (
        "fr",
        [
            "J'ai encore oublié mon parapluie à la maison et je suis arrivé trempé au travail.",
            "La boulangerie du coin vend le meilleur pain du quartier, mais elle ferme tôt.",
            "Tu peux me rappeler quand tu as une minute ? C'est à propos de la réunion de demain.",
            "Nous avons passé tout le week-end à peindre la cuisine et elle n'est toujours pas finie.",
            "Mon frère a déménagé dans un petit village l'année dernière et il ne veut plus revenir en ville.",
            "N'oublie pas d'arroser les plantes pendant que je suis en vacances.",
            "Le train avait du retard ce matin à cause d'un problème de signalisation.",
            "Elle apprend à jouer du piano depuis l'âge de sept ans.",
            "S'il ne pleut pas samedi, on pourrait pique-niquer au bord de la rivière.",
            "Je lis généralement quelques pages d'un livre avant de m'endormir.",
        ],
    ),
    (
        "de",
        [
            "Ich habe schon wieder meinen Regenschirm zu Hause vergessen und kam völlig durchnässt zur Arbeit.",
            "Die Bäckerei an der Ecke verkauft das beste Brot der Stadt, aber sie schließt früh.",
            "Kannst du mich zurückrufen, wenn du kurz Zeit hast? Es geht um das Treffen morgen.",
            "Wir haben das ganze Wochenende die Küche gestrichen und sie sieht immer noch unfertig aus.",
            "Mein Bruder ist letztes Jahr in ein kleines Dorf gezogen und will nie wieder in die Stadt.",
            "Denk bitte daran, die Pflanzen zu gießen, während ich im Urlaub bin.",
            "Der Zug hatte heute Morgen Verspätung wegen einer Störung an den Signalen.",
            "Sie lernt Klavier spielen, seit sie sieben Jahre alt ist.",
            "Wenn es am Samstag nicht regnet, könnten wir am Fluss picknicken.",
            "Abends lese ich meistens ein paar Seiten in einem Buch, bevor ich einschlafe.",
        ],
    ),
    (
        "it",
        [
            "Ho dimenticato di nuovo l'ombrello a casa e sono arrivato al lavoro tutto bagnato.",
            "Il panificio all'angolo vende il pane migliore del quartiere, ma chiude presto.",
            "Puoi richiamarmi quando hai un minuto? Si tratta della riunione di domani.",
            "Abbiamo passato tutto il fine settimana a dipingere la cucina e sembra ancora incompleta.",
            "Mio fratello si è trasferito in un piccolo paese l'anno scorso e dice che non tornerà mai in città.",
            "Ricordati di annaffiare le piante mentre sono in vacanza.",
            "Stamattina il treno era in ritardo per un guasto ai segnali.",
            "Lei studia pianoforte da quando aveva sette anni.",
            "Se sabato non piove, potremmo fare un picnic vicino al fiume.",
            "Di solito leggo qualche pagina di un libro prima di addormentarmi.",
        ],
    ),
    (
        "ca",
        [
            "M'he tornat a deixar el paraigua a casa i he arribat xop a la feina.",
            "El forn de la cantonada ven el millor pa del barri, però tanca d'hora.",
            "Em pots trucar quan tinguis un moment? És sobre la reunió de demà.",
            "Hem passat tot el cap de setmana pintant la cuina i encara sembla per acabar.",
            "El meu germà es va traslladar a un poble petit l'any passat i diu que no tornarà mai a la ciutat.",
            "Recorda't de regar les plantes mentre sóc de vacances.",
            "Aquest matí el tren anava amb retard per un problema amb els senyals.",
            "Ella aprèn a tocar el piano des que tenia set anys.",
            "Si dissabte no plou, podríem anar a dinar a la vora del riu.",
            "Normalment llegeixo unes quantes pàgines d'un llibre abans d'adormir-me.",
        ],
    ),
    (
        "gl",
        [
            "Esquecín outra vez o paraugas na casa e cheguei empapado ao traballo.",
            "A panadaría da esquina vende o mellor pan do barrio, pero pecha cedo.",
            "Podes chamarme cando teñas un momento? É sobre a xuntanza de mañá.",
            "Pasamos a fin de semana enteira pintando a cociña e aínda parece sen rematar.",
            "O meu irmán mudouse a unha aldea pequena o ano pasado e di que nunca volverá á cidade.",
            "Lembra regar as plantas mentres estou de vacacións.",
            "Esta mañá o tren chegou tarde por un problema cos sinais.",
            "Ela aprende a tocar o piano desde que tiña sete anos.",
            "Se non chove o sábado, poderiamos facer unha merenda á beira do río.",
            "Adoito ler unhas cantas páxinas dun libro antes de durmir.",
        ],
    ),
    (
        "ro",
        [
            "Mi-am uitat iar umbrela acasă și am ajuns la serviciu ud leoarcă.",
            "Brutăria din colț vinde cea mai bună pâine din cartier, dar se închide devreme.",
            "Mă poți suna înapoi când ai un minut? E vorba despre ședința de mâine.",
            "Am petrecut tot weekendul vopsind bucătăria și tot pare neterminată.",
            "Fratele meu s-a mutat într-un sat mic anul trecut și spune că nu se mai întoarce la oraș.",
            "Te rog să uzi plantele cât sunt în concediu.",
            "Trenul a întârziat azi-dimineață din cauza unei probleme la semnalizare.",
            "Ea învață să cânte la pian de când avea șapte ani.",
            "Dacă nu plouă sâmbătă, am putea face un picnic lângă râu.",
            "De obicei citesc câteva pagini dintr-o carte înainte să adorm.",
        ],
    ),
    (
        "pl",
        [
            "Znowu zapomniałem parasola z domu i dotarłem do pracy całkiem przemoczony.",
            "Piekarnia na rogu sprzedaje najlepszy chleb w okolicy, ale wcześnie zamyka.",
            "Możesz do mnie oddzwonić, kiedy będziesz miał chwilę? Chodzi o jutrzejsze spotkanie.",
            "Spędziliśmy cały weekend na malowaniu kuchni i nadal wygląda na niedokończoną.",
            "Mój brat przeprowadził się w zeszłym roku do małej wsi i mówi, że nigdy nie wróci do miasta.",
            "Pamiętaj, żeby podlewać rośliny, kiedy będę na urlopie.",
            "Pociąg spóźnił się dziś rano z powodu awarii sygnalizacji.",
            "Ona uczy się grać na pianinie, odkąd skończyła siedem lat.",
            "Jeśli w sobotę nie będzie padać, moglibyśmy zrobić piknik nad rzeką.",
            "Zwykle czytam kilka stron książki, zanim zasnę.",
        ],
    ),
    (
        "vi",
        [
            "Tôi lại quên ô ở nhà nên đến chỗ làm thì ướt sũng.",
            "Tiệm bánh ở góc phố bán bánh mì ngon nhất khu này, nhưng đóng cửa sớm.",
            "Khi nào rảnh bạn gọi lại cho tôi được không? Chuyện cuộc họp ngày mai.",
            "Chúng tôi dành cả cuối tuần để sơn nhà bếp mà trông vẫn chưa xong.",
            "Anh trai tôi chuyển về một ngôi làng nhỏ năm ngoái và nói rằng sẽ không bao giờ quay lại thành phố.",
            "Nhớ tưới cây giúp tôi trong lúc tôi đi nghỉ nhé.",
            "Sáng nay tàu đến muộn vì có sự cố với hệ thống tín hiệu.",
            "Cô ấy học chơi đàn piano từ khi lên bảy tuổi.",
            "Nếu thứ bảy trời không mưa, chúng ta có thể đi dã ngoại bên bờ sông.",
            "Tôi thường đọc vài trang sách trước khi đi ngủ.",
        ],
    ),
    (
        "ru",
        [
            "Я опять забыл зонтик дома и пришёл на работу совсем мокрым.",
            "Пекарня на углу продаёт лучший хлеб в районе, но рано закрывается.",
            "Можешь перезвонить мне, когда будет минутка? Это насчёт завтрашней встречи.",
            "Мы потратили все выходные на покраску кухни, а она до сих пор выглядит незаконченной.",
            "Мой брат в прошлом году переехал в маленькую деревню и говорит, что никогда не вернётся в город.",
            "Не забудь поливать цветы, пока я буду в отпуске.",
            "Сегодня утром поезд опоздал из-за неполадки с сигналами.",
            "Она учится играть на пианино с семи лет.",
            "Если в субботу не будет дождя, мы могли бы устроить пикник у реки.",
            "Перед сном я обычно читаю несколько страниц книги.",
        ],
    ),
    (
        "uk",
        [
            "Я знову забув парасольку вдома і прийшов на роботу зовсім мокрим.",
            "Пекарня на розі продає найкращий хліб у районі, але рано зачиняється.",
            "Можеш передзвонити мені, коли матимеш хвилинку? Це щодо завтрашньої зустрічі.",
            "Ми витратили всі вихідні на фарбування кухні, а вона досі виглядає незавершеною.",
            "Мій брат торік переїхав до маленького села і каже, що ніколи не повернеться до міста.",
            "Не забудь поливати квіти, поки я буду у відпустці.",
            "Сьогодні вранці потяг запізнився через несправність сигналів.",
            "Вона вчиться грати на піаніно з семи років.",
            "Якщо в суботу не буде дощу, ми могли б влаштувати пікнік біля річки.",
            "Перед сном я зазвичай читаю кілька сторінок книжки.",
        ],
    ),
];

/// Samples of 1, 2 and 5 consecutive sentences (wrapping), 10 of each length per language.
fn samples() -> Vec<(&'static str, usize, String)> {
    let mut out = Vec::new();
    for (tag, sentences) in CORPUS {
        for size in [1, 2, 5] {
            for start in 0..sentences.len() {
                let text: Vec<&str> = (0..size)
                    .map(|i| sentences[(start + i) % sentences.len()])
                    .collect();
                out.push((tag, size, text.join(" ")));
            }
        }
    }
    out
}

/// The default that makes the expected answer reachable (region of pt | en, Galician only through its default).
fn default_for(tag: &str) -> &'static str {
    match tag {
        "gl" => "gl",
        "pt-BR" => "pt-BR",
        _ => "en",
    }
}

/// Accuracy table of the detector on the corpus; run with `--nocapture` to read it. Raw guesses (no threshold) and
/// what passes the chosen floor, per sample length.
#[test]
fn detection_on_short_everyday_text_is_right_once_believed() {
    let ids = ids();
    let detector = Detector::with_allowlist(CANDIDATES.to_vec());
    let mut believed = [(0usize, 0usize); 3];
    let mut raw = [(0usize, 0usize); 3];
    let mut confusions: Vec<String> = Vec::new();
    for (tag, size, text) in samples() {
        let bucket = match size {
            1 => 0,
            2 => 1,
            _ => 2,
        };
        let default = default_for(tag);
        let guess = detector
            .detect(&text)
            .and_then(|info| id_of(info.lang(), &ids, default).map(|id| (id, info.confidence())));
        let raw_ok = guess.is_some_and(|(id, _)| id == tag);
        raw[bucket].1 += 1;
        raw[bucket].0 += usize::from(raw_ok);
        if !raw_ok {
            confusions.push(format!("{tag}/{size} -> {guess:?}"));
        }
        if let Detection::Found(id) = detect(&text, &ids, default) {
            believed[bucket].1 += 1;
            believed[bucket].0 += usize::from(id == tag);
            assert_eq!(id, tag, "a believed guess is wrong: {text}");
        }
        if tag == "gl" {
            // without a Galician default, Galician reads as its neighbors
            confusions.push(format!(
                "gl/{size} (default en) -> {:?}",
                detect(&text, &ids, "en")
            ));
        }
    }
    for (i, size) in [1, 2, 5].into_iter().enumerate() {
        println!(
            "{size} sentence(s): raw {}/{} right; believed {} of {} samples, {} right",
            raw[i].0, raw[i].1, believed[i].1, raw[i].1, believed[i].0
        );
    }
    println!("raw misses: {confusions:#?}");
    // five sentences of prose are enough to be believed nearly always
    assert!(believed[2].1 * 10 >= raw[2].1 * 9, "{believed:?}");
}

#[test]
fn two_region_languages_follow_the_default_region() {
    let ids = ids();
    assert_eq!(id_of(Lang::Por, &ids, "pt-PT"), Some("pt-PT"));
    assert_eq!(id_of(Lang::Por, &ids, "pt-BR"), Some("pt-BR"));
    assert_eq!(id_of(Lang::Por, &ids, "fr"), Some("pt-BR"));
    assert_eq!(id_of(Lang::Eng, &ids, "en-GB"), Some("en-GB"));
    assert_eq!(id_of(Lang::Eng, &ids, "de"), Some("en"));
    for (lang, id) in [
        (Lang::Spa, "es"),
        (Lang::Fra, "fr"),
        (Lang::Deu, "de"),
        (Lang::Ita, "it"),
        (Lang::Cat, "ca"),
        (Lang::Ron, "ro"),
        (Lang::Pol, "pl"),
        (Lang::Vie, "vi"),
        (Lang::Rus, "ru"),
        (Lang::Ukr, "uk"),
    ] {
        assert_eq!(id_of(lang, &ids, "en"), Some(id));
    }
    // Galician has no detector model: its writers' default turns the nearest guesses into Galician
    assert_eq!(id_of(Lang::Por, &ids, "gl"), Some("gl"));
    assert_eq!(id_of(Lang::Spa, &ids, "gl"), Some("gl"));
    assert_eq!(id_of(Lang::Spa, &ids, "en"), Some("es"));
    // no dictionary, no answer
    assert_eq!(id_of(Lang::Tur, &ids, "en"), None);
    assert_eq!(id_of(Lang::Rus, &["en"], "en"), None);
    assert_eq!(id_of(Lang::Ron, &["en"], "en"), None);
    assert_eq!(id_of(Lang::Eng, &["en-GB"], "en"), Some("en-GB"));
}

#[test]
fn short_or_unclear_text_keeps_the_default() {
    let ids = ids();
    assert_eq!(detect("", &ids, "en"), Detection::Unsure(None));
    assert_eq!(detect("123 456 -- !!", &ids, "en"), Detection::Unsure(None));
    assert_eq!(detect("Olá", &ids, "en"), Detection::Unsure(None));
    // long enough to measure, too close to call between two neighbors
    let unclear = detect("casa nova perto do rio", &ids, "en");
    assert!(matches!(unclear, Detection::Unsure(_)), "{unclear:?}");
}

#[test]
fn clear_text_is_found() {
    let ids = ids();
    let text = CORPUS[1].1[..5].join(" ");
    assert_eq!(detect(&text, &ids, "en"), Detection::Found("pt-BR"));
    assert_eq!(detect(&text, &ids, "pt-PT"), Detection::Found("pt-PT"));
    let german = CORPUS[4].1[..3].join(" ");
    assert_eq!(detect(&german, &ids, "pt-BR"), Detection::Found("de"));
    // the sample is cut, never refused
    let long = german.repeat(200);
    assert!(long.chars().count() > MAX_SAMPLE_CHARS);
    assert_eq!(detect(&long, &ids, "en"), Detection::Found("de"));
}

#[test]
fn scripts_without_a_dictionary_are_unsupported() {
    let ids = ids();
    assert_eq!(
        detect(
            "今日はとても良い天気ですね。散歩に行きましょう。",
            &ids,
            "en"
        ),
        Detection::Unsupported("kana")
    );
    assert_eq!(
        detect("오늘은 날씨가 정말 좋네요. 산책하러 갈까요?", &ids, "en"),
        Detection::Unsupported("hangul")
    );
    assert_eq!(
        detect("今天天气很好，我们去公园散步吧。", &ids, "en"),
        Detection::Unsupported("han")
    );
    assert_eq!(
        detect(
            "Σήμερα ο καιρός είναι πολύ ωραίος, πάμε μια βόλτα.",
            &ids,
            "en"
        ),
        Detection::Unsupported("greek")
    );
    // a script is covered only while one of its dictionaries is in the table
    assert_eq!(
        detect(
            "Сегодня очень хорошая погода, пойдём гулять.",
            &["en"],
            "en"
        ),
        Detection::Unsupported("cyrillic")
    );
}

#[test]
fn russian_and_ukrainian_are_told_apart() {
    let ids = ids();
    let russian = "Сегодня очень хорошая погода, поэтому мы пойдём гулять в парк.";
    let ukrainian = "Сьогодні дуже гарна погода, тому ми підемо гуляти в парк.";
    assert_eq!(detect(russian, &ids, "en"), Detection::Found("ru"));
    assert_eq!(detect(ukrainian, &ids, "en"), Detection::Found("uk"));
    // with only one of the two, the other reads as it: the default never decides between scripts
    assert_eq!(
        detect(ukrainian, &["en", "ru"], "en"),
        Detection::Found("ru")
    );
}

#[test]
fn the_ui_locale_gives_the_first_default() {
    let ids = ids();
    assert_eq!(locale_default("pt-BR", &ids), "pt-BR");
    assert_eq!(locale_default("pt-PT", &ids), "pt-PT");
    assert_eq!(locale_default("en", &ids), "en");
    assert_eq!(locale_default("en-GB", &ids), "en-GB");
    assert_eq!(locale_default("en-US", &ids), "en");
    assert_eq!(locale_default("pt", &ids), "pt-BR");
    assert_eq!(locale_default("fr-CA", &ids), "fr");
    for same in [
        "es", "fr", "de", "it", "ca", "gl", "ro", "pl", "ru", "uk", "vi",
    ] {
        assert_eq!(locale_default(same, &ids), same);
    }
    for other in ["ja", "ko", "zh-CN", "tr", "", "x"] {
        assert_eq!(locale_default(other, &ids), "en");
    }
    // a table without English still answers one of its own
    assert_eq!(locale_default("ja", &["ro"]), "ro");
}
