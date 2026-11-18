Correction pour certaines cartes graphiques AMD.

- Sur certains PC (carte AMD récente), l'encodeur de la carte graphique refusait les images de l'écran : Rewind relançait l'enregistrement en boucle sans rien garder. Désormais, si l'encodeur plante avant la première image, Rewind passe tout seul sur une méthode compatible (images copiées, toujours encodées par la carte graphique), puis sur le processeur en dernier recours.

---

Fix for some AMD graphics cards.

- On some PCs (recent AMD cards), the graphics card encoder rejected the screen images: Rewind kept restarting the recording without saving anything. Now, if the encoder fails before the first frame, Rewind automatically switches to a compatible method (copied frames, still encoded by the graphics card), then to the processor as a last resort.
