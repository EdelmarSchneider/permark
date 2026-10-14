// Runs before every test file. The interface language follows the machine ("System default"), so a test run on a
// Brazilian laptop would read Portuguese; tests that read UI strings expect English unless they choose a language.
import { language } from "../lib/i18n";

language.preference = "en";
