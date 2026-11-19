RustTerm sera un terminal SSH. Son but sera de pouvoir créer et gérer des connexions SSH dans une belle interface personnalisable, minimaliste.

# Interface

J'aimerais qu'elle soit simple, personnalisable et puisse au choix reprendre des éléments natifs du système (Swift, GTK, QT)

# Gestion des hotes

Je dois pouvoir dans une arborescence pouvoir ajouter des hotes et les rangers par dossier, je dois pouvoir leur ajouter aussi des tags.
Je voudrais aussi pouvoir sauvegarder mes hotes, personnaliser les icones des hotes (qu'on ai déjà un set d'icones d'intégré), sauvegarder nom d'hote, ip, mot de passe (ou clé), port ssh, description etc....

# Connexion

Je dois pouvoir me connecter via clé ssh et mot de passe. Pour la clé je voudrais une intégration de Bitwarden, Proton Pass, KeePass XC, etc... tout ce qui se trouve dans le .zshrc ou .bashrc ou avec fish pour une intégration de l'agent SSH natif sans à devoir installer ou ajouter en plus des clés. On peut toujours manuellement ajouter/importer des clés.

# Intégrations

Pour les hotes je voudrais avoir une barre de recherche, un système de tags, une bonne barre de menus avec de bons menus correctement intégrés à macOS... je voudrais aussi un bon terminal personnalisable où je peux tout personnaliser via GUI ou fichier de conf (documente le) et aussi je veux avoir un pannel à droite pour avoir du sftp et aussi des stats sur le serveur (usage cpu, mémoire, disque, réseau et load average).

# Personnalisation

Je voudrais pouvoir personnaliser complètement l'interface du terminal et du programme de manière générale comme la police de l'interface classique et la police de l'interface du terminal ainsi que les couleurs. Intègre déjà plusieurs thèmes ainsi que ma DA Midnights. Je voudrais que ça reste un terminal SSH très complet et personnalisable et léger.

# Comportements sur le système

Ajoute dans macOS, une icone dans la barre du système où au clic ça ouvre un petit menu avec quelques hotes ssh où au clic ça ouvre la fenêtre et ça se connecte, pour cette intégration fais attention au dynamic island sur Mac. Sur macOS toujours, l'application une fois fermée devra s'exéctuer en arrière plan pour une réouverture plus rapide. L'icone dans la barre de menus reste optionelle et désactivable, pareil pour l'exécution en arrière plan.
Ce menu aussi tu me l'ajoute dans la zone de notifications sur GNOME, KDE et Windows en utilisant des composants natifs (macOS aussi devra utiliser des composants natifs).

Sur Linux je voudrais que ce soit compatible wayland et je ne veux pas voir d'icone jaune "W" de Wayland.

Ajoute dans les options du split de fenêtre (vers la droite, gauche, bas, haut) comme si c'était un multiplex et le fait de pouvoir switcher facilement entre chaque hote, j'aimerais aussi du coup avoir des raccourcis clavier et les personnaliser (prends bien en compte entre macOS, Windows et Linux la layout du clavier entre les cmd, option etc...).

# Système de mise à jour

On devra ajouter un système de mise à jour automatisé et manuel. En gros à chaque lancement et chaque jour on va vérifier les mises à jour de disponible et si c'est le cas on notifie et propose l'installation de la mise à jour. Pour information ça se fera via le repo Github comme d'hab.

# Branches (git)

On va travailler sur trois branches qu'on va devoir créer :
dev : branche de dev par défaut
nightly : branche expérimentale et très instable où il pourrait y avoir chaque jour de nouvelles releases
beta : branche instable
stable : branche stable

Tu devra créer les workflow Github Actions pour ces branches là.
